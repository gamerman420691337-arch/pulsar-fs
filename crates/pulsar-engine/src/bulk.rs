//! Separate bounded current-user bulk endpoint; descriptors are not capabilities.
use crate::Engine;
use anyhow::Result;
use pulsar_protocol::*;
use std::sync::Arc;
use std::time::{Duration, Instant};
const IO_DEADLINE: Duration = Duration::from_secs(10);

#[cfg(unix)]
pub(crate) fn start(engine: Arc<Engine>) -> Result<crate::daemon::Endpoint> {
    let path = engine.config.bulk_endpoint();
    crate::daemon::Endpoint::start(engine, &path, "pulsar-bulk", 16, connection)
}
#[cfg(unix)]
fn connection(engine: &Arc<Engine>, stream: &mut LocalStream) {
    if engine.is_shutting_down() {
        return;
    }
    let handshake: BulkHandshakeEnvelope = {
        let mut io = DeadlineStream::new(stream, Instant::now() + IO_DEADLINE);
        match read_bulk_header(&mut io) {
            Ok(value) => value,
            Err(_) => return,
        }
    };
    if engine.is_shutting_down() {
        return;
    }
    match handshake {
        BulkHandshakeEnvelope::LiveCapture(h) => live_connection(engine, stream, h),
        BulkHandshakeEnvelope::PackageUpload(h) => import_connection(engine, stream, h),
        BulkHandshakeEnvelope::Motion(h) => motion_connection(engine, stream, h),
        BulkHandshakeEnvelope::Package(h) => package_connection(engine, stream, h),
        BulkHandshakeEnvelope::SourceMedia(h) => source_media_connection(engine, stream, h),
    }
}

fn motion_connection(engine: &Arc<Engine>, stream: &mut LocalStream, handshake: BulkHandshake) {
    let mut io = DeadlineStream::new(stream, Instant::now() + IO_DEADLINE);
    match engine.bulk_ready(&handshake) {
        Ok(lease) => {
            if write_bulk_header(&mut io, &BulkReply::Ready { lease }).is_err() {
                return;
            }
        }
        Err(error) => {
            let _ = write_bulk_header(&mut io, &BulkReply::Error(error));
            return;
        }
    }
    while !engine.is_shutting_down() {
        let mut io = DeadlineStream::new(stream, Instant::now() + IO_DEADLINE);
        let operation: BulkOperation = match read_bulk_header(&mut io) {
            Ok(value) => value,
            Err(_) => break,
        };
        if let Err(error) = operation.validate() {
            let _ = write_bulk_header(&mut io, &BulkReply::Error(error));
            break;
        }
        let result = match operation {
            BulkOperation::Upload { offset, byte_len } => {
                let bytes = match read_artifact_chunk(&mut io) {
                    Ok(bytes) => bytes,
                    Err(_) => break,
                };
                if bytes.len() != byte_len as usize {
                    let _ = write_bulk_header(
                        &mut io,
                        &BulkReply::Error(ProtocolError::invalid(
                            "bulk header and chunk length disagree",
                        )),
                    );
                    break;
                }
                match engine.bulk_upload(&handshake, offset, &bytes) {
                    Ok(accepted_prefix) => {
                        write_bulk_header(&mut io, &BulkReply::Uploaded { accepted_prefix })
                    }
                    Err(error) => {
                        let _ = write_bulk_header(&mut io, &BulkReply::Error(error));
                        break;
                    }
                }
            }
            BulkOperation::Download { offset, byte_len } => {
                match engine.bulk_download(&handshake, offset, byte_len) {
                    Ok(bytes) => {
                        write_bulk_header(&mut io, &BulkReply::Download { offset, byte_len })
                            .and_then(|_| write_artifact_chunk(&mut io, &bytes))
                    }
                    Err(error) => {
                        let _ = write_bulk_header(&mut io, &BulkReply::Error(error));
                        break;
                    }
                }
            }
        };
        if result.is_err() {
            break;
        }
    }
}
fn package_connection(
    engine: &Arc<Engine>,
    stream: &mut LocalStream,
    handshake: PackageBulkHandshake,
) {
    let mut io = DeadlineStream::new(stream, Instant::now() + IO_DEADLINE);
    let (mut connection, lease) = match engine.package_bulk_open(&handshake) {
        Ok(value) => value,
        Err(error) => {
            let _ = write_bulk_header(&mut io, &PackageBulkReply::Error(error));
            return;
        }
    };
    if write_bulk_header(
        &mut io,
        &PackageBulkReply::Ready {
            lease: (lease).into(),
        },
    )
    .is_err()
    {
        return;
    }
    while !engine.is_shutting_down() {
        let mut io = DeadlineStream::new(stream, Instant::now() + IO_DEADLINE);
        let request: PackageBulkRequest = match read_bulk_header(&mut io) {
            Ok(value) => value,
            Err(_) => break,
        };
        let (bytes, next_offset) = match engine.package_bulk_read(
            &mut connection,
            &handshake,
            request.offset,
            request.byte_len,
        ) {
            Ok(value) => value,
            Err(error) => {
                let _ = write_bulk_header(&mut io, &PackageBulkReply::Error(error));
                break;
            }
        };
        // Authorization was rechecked after the file read. Already admitted
        // bytes cannot be retracted after entering the transport.
        if write_bulk_header(
            &mut io,
            &PackageBulkReply::Download {
                offset: request.offset,
                byte_len: request.byte_len,
                next_offset,
            },
        )
        .and_then(|_| write_artifact_chunk(&mut io, &bytes))
        .is_err()
        {
            break;
        }
        if engine
            .package_bulk_delivered(&connection, &handshake, request.offset, request.byte_len)
            .is_err()
        {
            break;
        }
    }
}

fn import_connection(
    engine: &Arc<Engine>,
    stream: &mut LocalStream,
    handshake: PackageUploadHandshake,
) {
    let mut io = DeadlineStream::new(stream, Instant::now() + IO_DEADLINE);
    let (connection, lease) = match engine.import_bulk_open(&handshake) {
        Ok(value) => value,
        Err(error) => {
            let _ = write_bulk_header(&mut io, &PackageUploadReply::Error(error));
            return;
        }
    };
    if write_bulk_header(&mut io, &PackageUploadReply::Ready { lease }).is_err() {
        return;
    }
    while !engine.is_shutting_down() {
        let mut io = DeadlineStream::new(stream, Instant::now() + IO_DEADLINE);
        let request: PackageUploadRequest = match read_bulk_header(&mut io) {
            Ok(value) => value,
            Err(_) => break,
        };
        if let Err(error) = request.end() {
            let _ = write_bulk_header(&mut io, &PackageUploadReply::Error(error));
            break;
        }
        let bytes = match read_artifact_chunk(&mut io) {
            Ok(value) => value,
            Err(_) => break,
        };
        use sha2::{Digest, Sha256};
        if bytes.len() != request.byte_len as usize
            || format!("{:x}", Sha256::digest(&bytes)) != request.sha256
        {
            let _ = write_bulk_header(
                &mut io,
                &PackageUploadReply::Error(ProtocolError::invalid(
                    "upload header differs from payload bytes",
                )),
            );
            break;
        }
        let next_offset =
            match engine.import_bulk_append(&connection, &handshake, request.offset, &bytes) {
                Ok(value) => value,
                Err(error) => {
                    let _ = write_bulk_header(&mut io, &PackageUploadReply::Error(error));
                    break;
                }
            };
        if write_bulk_header(
            &mut io,
            &PackageUploadReply::Uploaded {
                offset: request.offset,
                byte_len: request.byte_len,
                sha256: request.sha256,
                next_offset,
            },
        )
        .is_err()
        {
            break;
        }
    }
}

fn live_connection(engine: &Arc<Engine>, stream: &mut LocalStream, handshake: LiveBulkHandshake) {
    if handshake.channel == LiveBulkChannel::LiveEncodedSource {
        encoded_connection(engine, stream, handshake);
        return;
    }
    use std::io::Read;
    let mut io = DeadlineStream::new(stream, Instant::now() + IO_DEADLINE);
    let ready = match engine.live_bulk_ready(&handshake, None) {
        Ok(report) => LiveBulkReply::Ready(report),
        Err(error) => {
            let _ = write_bulk_header(&mut io, &LiveBulkReply::Error(error));
            return;
        }
    };
    if write_bulk_header(&mut io, &ready).is_err() {
        return;
    }
    while !engine.is_shutting_down() {
        let mut io = DeadlineStream::new(stream, Instant::now() + IO_DEADLINE);
        let header: LiveFrameHeader = match read_bulk_header(&mut io) {
            Ok(header) => header,
            Err(_) => return,
        };
        if let Err(error) = engine.live_bulk_ready(&handshake, Some(&header)) {
            let _ = write_bulk_header(&mut io, &LiveBulkReply::Error(error));
            return;
        }
        if write_bulk_header(
            &mut io,
            &LiveBulkReply::SendPixels {
                frame: header.frame,
            },
        )
        .is_err()
        {
            return;
        }
        // Fixed stack allocation: a producer cannot select a body size. This
        // connection buffer is outside the separately admitted retained pixels.
        let mut pixels = [0u8; LIVE_FRAME_BYTES];
        if io.read_exact(&mut pixels).is_err() {
            return;
        }
        let reply = match engine.live_bulk_upload(&handshake, &header, &pixels) {
            Ok((report, replayed)) => LiveBulkReply::Accepted {
                frame: header.frame,
                replayed,
                report,
            },
            Err(error) => {
                let _ = write_bulk_header(&mut io, &LiveBulkReply::Error(error));
                return;
            }
        };
        if write_bulk_header(&mut io, &reply).is_err() {
            return;
        }
    }
}
fn encoded_connection(
    engine: &Arc<Engine>,
    stream: &mut LocalStream,
    handshake: LiveBulkHandshake,
) {
    use std::io::Read;
    let mut io = DeadlineStream::new(stream, Instant::now() + IO_DEADLINE);
    let mut connection = match engine.encoded_source_open(&handshake) {
        Ok(c) => c,
        Err(error) => {
            let _ = write_bulk_header(&mut io, &LiveEncodedReply::Error(error));
            return;
        }
    };
    let ready = match connection.ready() {
        Ok(report) => report,
        Err(error) => {
            let _ = write_bulk_header(&mut io, &LiveEncodedReply::Error(error));
            return;
        }
    };
    if write_bulk_header(&mut io, &LiveEncodedReply::Ready(ready)).is_err() {
        return;
    }
    while !engine.is_shutting_down() {
        let mut io = DeadlineStream::new(stream, Instant::now() + IO_DEADLINE);
        let request: LiveEncodedRequest = match read_bulk_header(&mut io) {
            Ok(r) => r,
            Err(_) => return,
        };
        if let Err(error) = connection.validate(&request) {
            connection.reject(error.clone());
            let _ = write_bulk_header(&mut io, &LiveEncodedReply::Error(error));
            return;
        }
        match &request {
            LiveEncodedRequest::Push {
                sequence, byte_len, ..
            } => {
                if write_bulk_header(
                    &mut io,
                    &LiveEncodedReply::SendBytes {
                        sequence: *sequence,
                    },
                )
                .is_err()
                {
                    return;
                }
                // Fixed bounded connection buffer; sender cannot choose an allocation.
                let mut bytes = [0u8; 65536];
                if io.read_exact(&mut bytes[..*byte_len as usize]).is_err() {
                    return;
                }
                let reply = match connection.push(&request, &bytes[..*byte_len as usize]) {
                    Ok(report) => LiveEncodedReply::Accepted {
                        sequence: *sequence,
                        input_bytes: report
                            .encoded_source
                            .as_ref()
                            .expect("admitted")
                            .input_bytes,
                        report,
                    },
                    Err(error) => {
                        connection.reject(error.clone());
                        let _ = write_bulk_header(&mut io, &LiveEncodedReply::Error(error));
                        return;
                    }
                };
                if write_bulk_header(&mut io, &reply).is_err() {
                    return;
                }
            }
            LiveEncodedRequest::Finish { .. } => {
                let reply = match connection.finish(&request) {
                    Ok(report) => LiveEncodedReply::Finished(report),
                    Err(error) => LiveEncodedReply::Error(error),
                };
                let _ = write_bulk_header(&mut io, &reply);
                return;
            }
        }
    }
}

fn source_media_connection(
    engine: &Arc<Engine>,
    stream: &mut LocalStream,
    handshake: SourceMediaBulkHandshake,
) {
    use sha2::{Digest, Sha256};
    let mut io = DeadlineStream::new(stream, Instant::now() + IO_DEADLINE);
    let (mut connection, binding) = match engine.source_media_bulk_open(&handshake) {
        Ok(value) => value,
        Err(error) => {
            let _ = write_bulk_header(&mut io, &SourceMediaBulkReply::Error(error));
            return;
        }
    };
    if write_bulk_header(
        &mut io,
        &SourceMediaBulkReply::Ready {
            binding: Box::new(binding),
        },
    )
    .is_err()
    {
        return;
    }
    while !engine.is_shutting_down() {
        let mut io = DeadlineStream::new(stream, Instant::now() + IO_DEADLINE);
        let request: SourceMediaBulkRequest = match read_bulk_header(&mut io) {
            Ok(value) => value,
            Err(_) => break,
        };
        let bytes = match engine.source_media_bulk_read(
            &mut connection,
            &handshake,
            request.offset,
            request.byte_len,
        ) {
            Ok(bytes) => bytes,
            Err(error) => {
                let _ = write_bulk_header(&mut io, &SourceMediaBulkReply::Error(error));
                break;
            }
        };
        // Exact bytes and current authority were checked after read, before any
        // body is released. Already written bytes cannot be retracted by revoke.
        if write_bulk_header(
            &mut io,
            &SourceMediaBulkReply::Download {
                offset: request.offset,
                byte_len: request.byte_len,
                sha256: format!("{:x}", Sha256::digest(&bytes)),
            },
        )
        .and_then(|_| write_artifact_chunk(&mut io, &bytes))
        .is_err()
        {
            break;
        }
    }
}
