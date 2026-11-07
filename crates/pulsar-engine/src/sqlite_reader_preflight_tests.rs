use super::*;
use rusqlite::Connection;
use std::io::Cursor;

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn page(version: u32, size: usize) -> Vec<u8> {
    let mut page = vec![0; size];
    page[..16].copy_from_slice(DATABASE_MAGIC);
    page[16..18].copy_from_slice(&(if size == 65536 { 1u16 } else { size as u16 }).to_be_bytes());
    page[60..64].copy_from_slice(&version.to_be_bytes());
    page
}
fn golden(big: bool) -> Vec<u8> {
    // Independently calculated fixed vectors for a 512-byte page-one commit.
    let (header, frame) = if big {
        (
            "377f0683002de218000002000000000001020304a0b0c0d01706cddb5e906864",
            "000000010000000101020304a0b0c0d0e66fbb6c39f1546c",
        )
    } else {
        (
            "377f0682002de218000002000000000001020304a0b0c0d0d8ce051461688d59",
            "000000010000000101020304a0b0c0d0cc20fb994ffa3bf2",
        )
    };
    [bytes(header), bytes(frame), page(43, 512)].concat()
}
fn version(raw: &[u8]) -> Result<Option<u32>> {
    committed_wal_version(&mut Cursor::new(raw), raw.len() as u64)
}
fn append(wal: &mut Vec<u8>, page_no: u32, commit: u32, data: &[u8]) {
    let big = wal[3] & 1 != 0;
    let previous = if wal.len() == 32 {
        &wal[24..32]
    } else {
        &wal[wal.len() - data.len() - 8..wal.len() - data.len()]
    };
    let sum = [be(&previous[..4]), be(&previous[4..])];
    let mut header = Vec::new();
    header.extend(page_no.to_be_bytes());
    header.extend(commit.to_be_bytes());
    header.extend_from_slice(&wal[16..24]);
    let sum = checksum(data, big, checksum(&header[..8], big, sum));
    header.extend(sum[0].to_be_bytes());
    header.extend(sum[1].to_be_bytes());
    wal.extend(header);
    wal.extend_from_slice(data);
}

#[test]
fn checksum_word_order_and_fixed_wal_vectors_match() {
    for big in [false, true] {
        let raw: Vec<_> = [1u32, 2, 3, 4]
            .into_iter()
            .flat_map(|v| {
                if big {
                    v.to_be_bytes()
                } else {
                    v.to_le_bytes()
                }
            })
            .collect();
        assert_eq!(checksum(&raw, big, [0, 0]), [7, 14]);
        assert_eq!(checksum(&raw, big, [9, 10]), [55, 91]);
        assert_eq!(version(&golden(big)).unwrap(), Some(43));
    }
}

#[test]
fn only_committed_valid_prefix_updates_the_reader_version() {
    let mut wal = golden(false);
    append(&mut wal, 1, 0, &page(99, 512));
    assert_eq!(version(&wal).unwrap(), Some(43));
    append(&mut wal, 2, 2, &vec![0; 512]);
    assert_eq!(version(&wal).unwrap(), Some(99));
    append(&mut wal, 1, 2, &page(40, 512));
    assert_eq!(version(&wal).unwrap(), Some(40)); // SQLite reads the latest committed page one.
    let prefix = wal.clone();
    append(&mut wal, 1, 2, &page(100, 512));
    for fault in ["salt", "checksum", "zero_page", "partial"] {
        let mut changed = wal.clone();
        match fault {
            "salt" => changed[prefix.len() + 8] ^= 1,
            "checksum" => *changed.last_mut().unwrap() ^= 1,
            "zero_page" => changed[prefix.len()..prefix.len() + 4].fill(0),
            _ => {
                changed.pop();
            }
        }
        assert_eq!(version(&changed).unwrap(), Some(40), "{fault}");
    }
    for n in 0..golden(false).len() {
        assert_eq!(
            version(&golden(false)[..n]).unwrap(),
            None,
            "torn length {n}"
        );
    }
}

#[test]
fn malformed_header_never_allocates_an_unbounded_page_or_admits_a_format() {
    let original = golden(false);
    for (offset, value) in [(8, 0), (8, 1), (8, 513), (8, u32::MAX), (4, WAL_FORMAT + 1)] {
        let mut changed = original.clone();
        changed[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        let sum = checksum(&changed[..24], false, [0, 0]);
        changed[24..28].copy_from_slice(&sum[0].to_be_bytes());
        changed[28..32].copy_from_slice(&sum[1].to_be_bytes());
        assert!(version(&changed).is_err());
    }
    let mut corrupt = original;
    corrupt[24] ^= 1;
    assert_eq!(version(&corrupt).unwrap(), None);
}

#[test]
fn real_sqlite_wal_page_sizes_and_commit_on_another_page_are_read_without_effects() {
    for size in [512, 4096, 65536] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.sqlite3");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(&format!("PRAGMA page_size={size};PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;CREATE TABLE probe(x);PRAGMA user_version=25;PRAGMA wal_checkpoint(TRUNCATE)")).unwrap();
        db.execute_batch(&format!(
            "BEGIN IMMEDIATE;PRAGMA user_version=43;INSERT INTO probe VALUES(zeroblob({}));COMMIT",
            size * 2
        ))
        .unwrap();
        let paths = [
            path,
            dir.path().join("projects.sqlite3-wal"),
            dir.path().join("projects.sqlite3-shm"),
        ];
        let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
        assert_eq!(be(&before[0][60..64]), 25);
        assert_eq!(version(&before[1]).unwrap(), Some(43));
        let frame_size = 24 + size;
        assert!(before[1][32..]
            .chunks_exact(frame_size)
            .any(|f| be(&f[..4]) != 1 && be(&f[4..8]) != 0));
        assert!(check(dir.path(), 42).is_err());
        check(dir.path(), 43).unwrap();
        assert_eq!(
            before,
            paths
                .iter()
                .map(|p| fs::read(p).unwrap())
                .collect::<Vec<_>>()
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }
}

#[test]
fn changed_or_replaced_files_refuse_the_observation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("probe");
    unchanged(&path, None).unwrap();
    fs::write(&path, b"original").unwrap();
    assert!(unchanged(&path, None).is_err());
    let held = optional_file(&path).unwrap().unwrap();
    fs::write(&path, b"different length").unwrap();
    assert!(unchanged(&path, Some(&held)).is_err());
    #[cfg(unix)]
    {
        let held = optional_file(&path).unwrap().unwrap();
        fs::rename(&path, dir.path().join("old")).unwrap();
        fs::write(&path, b"different length").unwrap();
        assert!(unchanged(&path, Some(&held)).is_err());
    }
}

#[cfg(unix)]
#[test]
fn wal_aliases_and_nonregular_files_are_not_opened() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("projects.sqlite3"), page(25, 512)).unwrap();
    let other = dir.path().join("other");
    fs::write(&other, golden(false)).unwrap();
    let wal = dir.path().join("projects.sqlite3-wal");
    symlink(&other, &wal).unwrap();
    assert!(check(dir.path(), 42).is_err());
    fs::remove_file(&wal).unwrap();
    fs::create_dir(&wal).unwrap();
    assert!(check(dir.path(), 42).is_err());
}
