//! Streaming ONNX dependency/tensor admission, never execution or a full checker.
//! Schema: https://github.com/onnx/onnx/blob/main/onnx/onnx.proto3
use anyhow::{bail, ensure, Context, Result};
use pulsar_protocol::{ModelDecoder, ModelInputContract, TensorLayout};
use std::io::{BufReader, Read, Seek, SeekFrom};

const MAX_FIELDS: usize = 100_000;
const MAX_DEPTH: usize = 24;
#[derive(Clone, Copy)]
enum Value {
    Integer(u64),
    Bytes(u64, u64),
    Fixed,
}
struct Field {
    tag: u32,
    value: Value,
}
struct Parser<R> {
    reader: R,
    fields: usize,
}
impl<R: Read + Seek> Parser<R> {
    fn varint(&mut self, end: u64) -> Result<u64> {
        let mut value = 0u64;
        for shift in (0..70).step_by(7) {
            ensure!(
                self.reader.stream_position()? < end,
                "truncated ONNX varint"
            );
            let mut byte = [0];
            self.reader.read_exact(&mut byte)?;
            ensure!(shift != 63 || byte[0] <= 1, "ONNX varint overflow");
            value |= u64::from(byte[0] & 127) << shift;
            if byte[0] & 128 == 0 {
                return Ok(value);
            }
        }
        bail!("ONNX varint overflow")
    }
    fn next(&mut self, end: u64) -> Result<Option<Field>> {
        let position = self.reader.stream_position()?;
        ensure!(position <= end, "ONNX message overrun");
        if position == end {
            return Ok(None);
        }
        self.fields += 1;
        ensure!(self.fields <= MAX_FIELDS, "ONNX field admission exceeded");
        let key = self.varint(end)?;
        let tag = u32::try_from(key >> 3)?;
        ensure!(tag > 0 && tag < 1 << 29, "invalid ONNX field number");
        let value = match key & 7 {
            0 => Value::Integer(self.varint(end)?),
            wire @ (1 | 2 | 5) => {
                let size = match wire {
                    1 => 8,
                    5 => 4,
                    _ => self.varint(end)?,
                };
                let start = self.reader.stream_position()?;
                let finish = start.checked_add(size).context("ONNX length overflow")?;
                ensure!(finish <= end, "truncated ONNX field");
                self.reader.seek(SeekFrom::Start(finish))?;
                if wire == 2 {
                    Value::Bytes(start, finish)
                } else {
                    Value::Fixed
                }
            }
            _ => bail!("unsupported ONNX wire type"),
        };
        Ok(Some(Field { tag, value }))
    }
    fn text(&mut self, value: Value) -> Result<String> {
        self.text_limit(value, 1024)
    }
    fn text_limit(&mut self, value: Value, limit: u64) -> Result<String> {
        let (start, end) = bytes(value)?;
        ensure!(end - start <= limit, "ONNX text exceeds admission");
        self.reader.seek(SeekFrom::Start(start))?;
        let mut text = vec![0; (end - start) as usize];
        self.reader.read_exact(&mut text)?;
        let text = String::from_utf8(text)?;
        ensure!(
            !text.chars().any(char::is_control),
            "invalid ONNX identifier"
        );
        Ok(text)
    }
    fn property(&mut self, value: Value) -> Result<(String, String)> {
        let (start, end) = bytes(value)?;
        self.reader.seek(SeekFrom::Start(start))?;
        let (mut key, mut value) = (None, None);
        while let Some(field) = self.next(end)? {
            match field.tag {
                1 => {
                    ensure!(key.is_none(), "duplicate ONNX metadata key field");
                    key = Some(self.text(field.value)?);
                }
                2 => {
                    ensure!(value.is_none(), "duplicate ONNX metadata value field");
                    value = Some(self.text_limit(field.value, 4096)?);
                }
                _ => bail!("unknown ONNX metadata field"),
            }
        }
        Ok((
            key.context("missing ONNX metadata key")?,
            value.context("missing ONNX metadata value")?,
        ))
    }
    fn walk(&mut self, kind: Kind, value: Value, depth: usize) -> Result<()> {
        ensure!(depth <= MAX_DEPTH, "ONNX nesting exceeds admission");
        let (start, end) = bytes(value)?;
        self.reader.seek(SeekFrom::Start(start))?;
        while let Some(field) = self.next(end)? {
            match (kind, field.tag) {
                (Kind::Graph, 1) => self.walk(Kind::Node, field.value, depth + 1)?,
                (Kind::Graph, 5) => self.walk(Kind::Tensor, field.value, depth + 1)?,
                (Kind::Graph, 15) => self.walk(Kind::Sparse, field.value, depth + 1)?,
                (Kind::Node, 5) => self.walk(Kind::Attribute, field.value, depth + 1)?,
                (Kind::Node, 7) => ensure!(
                    matches!(
                        self.text(field.value)?.as_str(),
                        "" | "ai.onnx" | "ai.onnx.ml"
                    ),
                    "custom ONNX domain is not admitted"
                ),
                (Kind::Node, 10) => bail!("multi-device ONNX nodes are not admitted"),
                (Kind::Attribute, 5 | 10) => self.walk(Kind::Tensor, field.value, depth + 1)?,
                (Kind::Attribute, 6 | 11) => self.walk(Kind::Graph, field.value, depth + 1)?,
                (Kind::Attribute, 22 | 23) => self.walk(Kind::Sparse, field.value, depth + 1)?,
                (Kind::Sparse, 1 | 2) => self.walk(Kind::Tensor, field.value, depth + 1)?,
                (Kind::Tensor, 13) => bail!("ONNX external tensor data is not admitted"),
                (Kind::Tensor, 14) => ensure!(
                    integer(field.value)? == 0,
                    "ONNX external tensor location is not admitted"
                ),
                // Known primitive/metadata fields cannot contain dependencies.
                (Kind::Graph, 2 | 10 | 11 | 12 | 13 | 14 | 16)
                | (Kind::Node, 1 | 2 | 3 | 4 | 6 | 8 | 9)
                | (Kind::Attribute, 1..=4 | 7..=9 | 13..=15 | 20 | 21)
                | (Kind::Sparse, 3)
                | (Kind::Tensor, 1..=12 | 16) => {}
                _ => bail!("unknown ONNX structural field is not admitted"),
            }
        }
        Ok(())
    }
    fn shape(&mut self, value: Value) -> Result<Vec<Option<u64>>> {
        let (start, end) = bytes(value)?;
        self.reader.seek(SeekFrom::Start(start))?;
        let mut dimensions = Vec::new();
        while let Some(field) = self.next(end)? {
            ensure!(
                field.tag == 1 && dimensions.len() < 16,
                "ONNX tensor rank exceeds admission"
            );
            let (a, b) = bytes(field.value)?;
            self.reader.seek(SeekFrom::Start(a))?;
            let mut dimension = None;
            let mut seen = false;
            while let Some(part) = self.next(b)? {
                match part.tag {
                    1 => {
                        ensure!(!seen, "duplicate ONNX dimension");
                        let n = integer(part.value)?;
                        ensure!(n > 0 && n <= i64::MAX as u64, "invalid ONNX dimension");
                        dimension = Some(n);
                        seen = true;
                    }
                    2 => {
                        ensure!(
                            !seen && !self.text(part.value)?.is_empty(),
                            "invalid symbolic dimension"
                        );
                        seen = true;
                    }
                    3 => {}
                    _ => bail!("unknown ONNX dimension field"),
                }
            }
            dimensions.push(dimension);
        }
        Ok(dimensions)
    }
    fn tensor_type(&mut self, value: Value) -> Result<Vec<Option<u64>>> {
        let (start, end) = bytes(value)?;
        let type_end = end;
        self.reader.seek(SeekFrom::Start(start))?;
        let mut tensor = None;
        while let Some(field) = self.next(end)? {
            match field.tag {
                1 => {
                    ensure!(tensor.is_none(), "duplicate ONNX tensor type");
                    tensor = Some(field.value);
                }
                6 => {}
                _ => bail!("non-tensor ONNX input/output is not admitted"),
            }
        }
        let (start, end) = bytes(tensor.context("missing ONNX tensor type")?)?;
        self.reader.seek(SeekFrom::Start(start))?;
        let mut element = None;
        let mut shape = None;
        while let Some(field) = self.next(end)? {
            match field.tag {
                1 => {
                    ensure!(element.is_none(), "duplicate ONNX element type");
                    element = Some(integer(field.value)?);
                }
                2 => {
                    ensure!(shape.is_none(), "duplicate ONNX tensor shape");
                    shape = Some(self.shape(field.value)?);
                }
                _ => bail!("unknown ONNX tensor type field"),
            }
        }
        ensure!(element == Some(1), "ONNX input/output must be float32");
        self.reader.seek(SeekFrom::Start(type_end))?;
        shape.context("missing ONNX tensor shape")
    }
    fn info(&mut self, value: Value) -> Result<(String, Vec<Option<u64>>)> {
        let (start, end) = bytes(value)?;
        self.reader.seek(SeekFrom::Start(start))?;
        let mut name = None;
        let mut shape = None;
        while let Some(field) = self.next(end)? {
            match field.tag {
                1 => {
                    ensure!(name.is_none(), "duplicate ONNX value name");
                    name = Some(self.text(field.value)?);
                }
                2 => {
                    ensure!(shape.is_none(), "duplicate ONNX value type");
                    shape = Some(self.tensor_type(field.value)?);
                }
                3 | 4 => {}
                _ => bail!("unknown ONNX value info field"),
            }
        }
        let name = name.context("missing ONNX value name")?;
        ensure!(!name.is_empty(), "missing ONNX value name");
        Ok((name, shape.context("missing ONNX value type")?))
    }
    fn opset(&mut self, value: Value) -> Result<String> {
        let (start, end) = bytes(value)?;
        self.reader.seek(SeekFrom::Start(start))?;
        let mut domain = None;
        let mut version = None;
        while let Some(field) = self.next(end)? {
            match field.tag {
                1 => {
                    ensure!(domain.is_none(), "duplicate ONNX opset domain");
                    domain = Some(self.text(field.value)?);
                }
                2 => {
                    ensure!(version.is_none(), "duplicate ONNX opset version");
                    version = Some(integer(field.value)?);
                }
                _ => bail!("unknown ONNX opset field"),
            }
        }
        let domain = domain.unwrap_or_default();
        ensure!(
            matches!(domain.as_str(), "" | "ai.onnx" | "ai.onnx.ml")
                && version.is_some_and(|v| v > 0 && v <= i64::MAX as u64),
            "unsupported ONNX opset declaration"
        );
        Ok(domain)
    }
}
#[derive(Clone, Copy)]
enum Kind {
    Graph,
    Node,
    Attribute,
    Tensor,
    Sparse,
}
fn bytes(value: Value) -> Result<(u64, u64)> {
    if let Value::Bytes(a, b) = value {
        Ok((a, b))
    } else {
        bail!("ONNX field requires a message")
    }
}
fn integer(value: Value) -> Result<u64> {
    if let Value::Integer(n) = value {
        Ok(n)
    } else {
        bail!("ONNX field requires an integer")
    }
}

pub(crate) fn validate<R: Read + Seek>(
    reader: R,
    len: u64,
    input: &ModelInputContract,
) -> Result<()> {
    validate_recipe(reader, len, input, false).map(|_| ())
}

#[derive(Debug)]
pub(crate) struct AnnotatedModelBinding {
    pub contract: pulsar_protocol::AnnotatedTemporalModelContract,
    pub training_intake_sha256: String,
}

pub(crate) fn validate_annotated<R: Read + Seek>(
    reader: R,
    len: u64,
    input: &ModelInputContract,
) -> Result<AnnotatedModelBinding> {
    ensure!(
        input.decoder == ModelDecoder::TemporalScriptPosition,
        "annotated recipe requires a temporal position tensor contract"
    );
    validate_recipe(reader, len, input, true)?.context("missing annotated model binding")
}

fn validate_recipe<R: Read + Seek>(
    reader: R,
    len: u64,
    input: &ModelInputContract,
    annotated: bool,
) -> Result<Option<AnnotatedModelBinding>> {
    ensure!(
        len > 0 && len <= 256 * 1024 * 1024,
        "ONNX model exceeds pack admission"
    );
    input.validate()?;
    let mut p = Parser {
        reader: BufReader::with_capacity(64 * 1024, reader),
        fields: 0,
    };
    let mut graph = None;
    let mut ir = None;
    let mut opsets = std::collections::BTreeSet::new();
    let mut metadata = std::collections::BTreeMap::new();
    while let Some(field) = p.next(len)? {
        match field.tag {
            1 => {
                ensure!(ir.is_none(), "duplicate ONNX IR version");
                ir = Some(integer(field.value)?);
            }
            7 => {
                ensure!(graph.is_none(), "duplicate ONNX graph");
                graph = Some(field.value);
            }
            8 => {
                ensure!(opsets.len() < 64, "ONNX opset admission exceeded");
                ensure!(
                    opsets.insert(p.opset(field.value)?),
                    "duplicate ONNX opset domain"
                );
            }
            14 if annotated => {
                ensure!(metadata.len() < 64, "ONNX metadata count exceeds admission");
                let (key, value) = p.property(field.value)?;
                ensure!(
                    !key.is_empty() && metadata.insert(key, value).is_none(),
                    "duplicate or empty ONNX metadata key"
                );
            }
            2..=6 | 14 => {}
            20 | 25 | 26 => {
                bail!("ONNX training/functions/multi-device configuration not admitted")
            }
            _ => bail!("unknown ONNX model field"),
        }
    }
    ensure!(
        ir.is_some_and(|v| (3..=13).contains(&v)) && !opsets.is_empty(),
        "missing or unsupported ONNX version/opsets"
    );
    let graph = graph.context("missing ONNX graph")?;
    p.walk(Kind::Graph, graph, 0)?;
    let (start, end) = bytes(graph)?;
    p.reader.seek(SeekFrom::Start(start))?;
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    while let Some(field) = p.next(end)? {
        match field.tag {
            11 => {
                ensure!(inputs.is_empty(), "ONNX requires exactly one input");
                inputs.push(p.info(field.value)?);
            }
            12 => {
                ensure!(outputs.is_empty(), "ONNX requires exactly one output");
                outputs.push(p.info(field.value)?);
            }
            _ => {}
        }
    }
    ensure!(
        inputs.len() == 1 && outputs.len() == 1,
        "missing ONNX input/output"
    );
    let expected = match input.layout {
        TensorLayout::Nchw => vec![
            1,
            input.channels as u64,
            input.height as u64,
            input.width as u64,
        ],
        TensorLayout::Nhwc => vec![
            1,
            input.height as u64,
            input.width as u64,
            input.channels as u64,
        ],
    };
    ensure!(
        inputs[0].1 == expected.into_iter().map(Some).collect::<Vec<_>>(),
        "ONNX input dimensions differ from profile"
    );
    let output = &outputs[0].1;
    match input.decoder {
        ModelDecoder::TemporalScriptPosition => ensure!(
            inputs[0].0
                == if annotated {
                    pulsar_protocol::ANNOTATED_TEMPORAL_INPUT_NAME
                } else {
                    "frame_pair"
                }
                && outputs[0].0 == "script_position"
                && output == &[Some(1), Some(1)],
            "ONNX position output differs from profile"
        ),
        ModelDecoder::YoloChannelMajor => ensure!(
            output.len() == 3
                && output[0] == Some(1)
                && output[1] == Some(input.class_count as u64 + 4),
            "ONNX channel-major output differs from profile"
        ),
        ModelDecoder::YoloEndToEnd => ensure!(
            output.len() == 3 && output[0] == Some(1) && output[2] == Some(6),
            "ONNX end-to-end output differs from profile"
        ),
    }
    if annotated {
        let contract: pulsar_protocol::AnnotatedTemporalModelContract = serde_json::from_str(
            metadata
                .get("pulsar.temporal_recipe")
                .context("missing annotated model recipe")?,
        )?;
        contract.validate()?;
        let digest = metadata
            .get("pulsar.training_intake_sha256")
            .context("missing annotated model intake identity")?;
        ensure!(
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "invalid annotated model intake identity"
        );
        Ok(Some(AnnotatedModelBinding {
            contract,
            training_intake_sha256: digest.clone(),
        }))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn v(mut n: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let b = (n & 127) as u8;
            n >>= 7;
            out.push(b | if n > 0 { 128 } else { 0 });
            if n == 0 {
                return out;
            }
        }
    }
    fn number(tag: u64, n: u64) -> Vec<u8> {
        [v(tag * 8), v(n)].concat()
    }
    fn message(tag: u64, data: Vec<u8>) -> Vec<u8> {
        [v(tag * 8 + 2), v(data.len() as u64), data].concat()
    }
    fn info(name: &str, shape: &[u64]) -> Vec<u8> {
        let shape = shape
            .iter()
            .flat_map(|n| message(1, number(1, *n)))
            .collect();
        [
            message(1, name.as_bytes().to_vec()),
            message(2, message(1, [number(1, 1), message(2, shape)].concat())),
        ]
        .concat()
    }
    fn contract() -> ModelInputContract {
        ModelInputContract {
            width: 64,
            height: 64,
            channels: 3,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: ModelDecoder::YoloChannelMajor,
        }
    }
    fn graph(extra: Vec<u8>) -> Vec<u8> {
        [
            message(11, info("rgb", &[1, 3, 64, 64])),
            message(12, info("boxes", &[1, 5, 1])),
            extra,
        ]
        .concat()
    }
    fn model(graph: Vec<u8>) -> Vec<u8> {
        [number(1, 8), message(8, number(2, 13)), message(7, graph)].concat()
    }
    fn check(data: &[u8], input: &ModelInputContract) -> Result<()> {
        validate(Cursor::new(data), data.len() as u64, input)
    }
    #[test]
    fn annotated_recipe_metadata_names_duplicates_and_truncation_are_checked() {
        use pulsar_protocol::AnnotatedTemporalModelContract;
        let input = ModelInputContract {
            width: 64,
            height: 64,
            channels: 6,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: ModelDecoder::TemporalScriptPosition,
        };
        let graph = [
            message(11, info("target_frame_pair", &[1, 6, 64, 64])),
            message(12, info("script_position", &[1, 1])),
        ]
        .concat();
        let base = model(graph);
        let property = |key: &str, value: &str| {
            message(
                14,
                [
                    message(1, key.as_bytes().to_vec()),
                    message(2, value.as_bytes().to_vec()),
                ]
                .concat(),
            )
        };
        let recipe = property(
            "pulsar.temporal_recipe",
            &serde_json::to_string(&AnnotatedTemporalModelContract::expected()).unwrap(),
        );
        let lineage = property("pulsar.training_intake_sha256", &"a".repeat(64));
        let data = [base.clone(), recipe.clone(), lineage.clone()].concat();
        validate_annotated(Cursor::new(&data), data.len() as u64, &input).unwrap();
        assert!(check(&data, &input).is_err());
        for bad in [
            base.clone(),
            [base.clone(), recipe.clone()].concat(),
            [data.clone(), recipe.clone()].concat(),
            [
                base,
                recipe,
                property("pulsar.training_intake_sha256", "unknown"),
            ]
            .concat(),
        ] {
            assert!(validate_annotated(Cursor::new(&bad), bad.len() as u64, &input).is_err());
        }
        for end in 0..data.len() {
            assert!(validate_annotated(Cursor::new(&data[..end]), end as u64, &input).is_err());
        }
    }
    #[test]
    fn tensor_contract_and_every_truncated_prefix_are_checked() {
        let data = model(graph(Vec::new()));
        check(&data, &contract()).unwrap();
        for end in 0..data.len() {
            assert!(
                check(&data[..end], &contract()).is_err(),
                "accepted prefix {end}"
            );
        }
        let mut bad = contract();
        bad.layout = TensorLayout::Nhwc;
        assert!(check(&data, &bad).is_err());
        bad = contract();
        bad.class_count = 2;
        assert!(check(&data, &bad).is_err());
        assert!(check(
            &model(
                [
                    message(11, info("rgb", &[1, 3, 64, 64])),
                    message(12, info("boxes", &[1, 5, 1])),
                    message(12, info("extra", &[1, 5, 1]))
                ]
                .concat()
            ),
            &contract()
        )
        .is_err());
        let position = ModelInputContract {
            width: 64,
            height: 64,
            channels: 6,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: ModelDecoder::TemporalScriptPosition,
        };
        check(
            &model(
                [
                    message(11, info("frame_pair", &[1, 6, 64, 64])),
                    message(12, info("script_position", &[1, 1])),
                ]
                .concat(),
            ),
            &position,
        )
        .unwrap();
        assert!(check(
            &model(
                [
                    message(11, info("wrong_pair", &[1, 6, 64, 64])),
                    message(12, info("script_position", &[1, 1]))
                ]
                .concat()
            ),
            &position
        )
        .is_err());
        // Type denotation after the tensor must not be read as ValueInfo fields.
        let shape = [1, 3, 64, 64]
            .iter()
            .flat_map(|n| message(1, number(1, *n)))
            .collect();
        let ty = [
            message(1, [number(1, 1), message(2, shape)].concat()),
            message(6, b"IMAGE".to_vec()),
        ]
        .concat();
        let input = [message(1, b"rgb".to_vec()), message(2, ty)].concat();
        check(
            &model([message(11, input), message(12, info("boxes", &[1, 5, 1]))].concat()),
            &contract(),
        )
        .unwrap();
    }
    #[test]
    fn external_dependencies_refuse_in_initializer_attribute_sparse_and_nested_graph() {
        for tensor in [message(13, Vec::new()), number(14, 1)] {
            let attribute = message(5, tensor.clone());
            let nested = message(6, message(5, tensor.clone()));
            for extra in [
                message(5, tensor.clone()),
                message(1, message(5, attribute)),
                message(15, message(2, tensor.clone())),
                message(1, message(5, nested)),
            ] {
                assert!(check(&model(graph(extra)), &contract()).is_err());
            }
        }
        check(&model(graph(message(5, number(14, 0)))), &contract()).unwrap();
    }
    #[test]
    fn unknown_dependency_paths_and_ambiguous_top_level_messages_refuse() {
        for extra in [
            message(20, Vec::new()),
            message(25, Vec::new()),
            message(26, Vec::new()),
            message(99, Vec::new()),
            message(7, graph(Vec::new())),
            number(1, 8),
            message(8, number(2, 13)),
        ] {
            assert!(check(&[model(graph(Vec::new())), extra].concat(), &contract()).is_err());
        }
        assert!(check(
            &model(graph(message(1, message(7, b"custom.native".to_vec())))),
            &contract()
        )
        .is_err());
        assert!(check(
            &[0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 2],
            &contract()
        )
        .is_err());
    }
    #[test]
    fn recursive_and_field_bombs_are_bounded() {
        let mut nested = Vec::new();
        for _ in 0..MAX_DEPTH {
            nested = message(1, message(5, message(6, nested)));
        }
        assert!(check(&model(graph(nested)), &contract()).is_err());
        assert!(check(
            &model(graph(message(10, Vec::new()).repeat(MAX_FIELDS))),
            &contract()
        )
        .is_err());
    }
}
