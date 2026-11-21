//! Reading the files of the original model: a tar archive (POSIX ustar) and, inside it, a
//! checkpoint of TensorFlow, which is an index in the table format of LevelDB, holding a
//! protocol buffer per tensor, and a data file with the values.
//!
//! Formats read from their public descriptions: the tar header of POSIX.1-2001 (`ustar`), the
//! table format of LevelDB (`doc/table_format.md` of google/leveldb) and the messages
//! `BundleEntryProto` and `TensorShapeProto` of TensorFlow (`tensor_bundle.proto`,
//! `tensor_shape.proto`). Every count and offset is checked against the bytes there are.

use std::collections::BTreeMap;

use crate::weights::{MAX_TENSORS, MAX_VALUES, Tensor, Tensors};

#[derive(Debug, PartialEq, Eq)]
pub struct BundleError(pub &'static str);

impl std::fmt::Display for BundleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "the archive of the model is damaged: {}", self.0)
    }
}

impl std::error::Error for BundleError {}

fn fail<T>(why: &'static str) -> Result<T, BundleError> {
    Err(BundleError(why))
}

/// The regular files of a tar archive by name. Folders, links and the rest are passed over.
pub fn untar(archive: &[u8]) -> Result<BTreeMap<String, &[u8]>, BundleError> {
    let mut files = BTreeMap::new();
    let mut at = 0;
    while at + 512 <= archive.len() {
        let header = &archive[at..at + 512];
        if header.iter().all(|byte| *byte == 0) {
            break;
        }
        let text = |range: std::ops::Range<usize>| {
            let field = &header[range];
            let end = field
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(field.len());
            String::from_utf8_lossy(&field[..end]).into_owned()
        };
        let size_text = text(124..136);
        let size = usize::from_str_radix(size_text.trim(), 8).or_else(|_| {
            if size_text.trim().is_empty() {
                Ok(0)
            } else {
                fail("a size in a header")
            }
        })?;
        let kind = header[156];
        let prefix = text(345..500);
        let name = if prefix.is_empty() {
            text(0..100)
        } else {
            format!("{prefix}/{}", text(0..100))
        };
        let start = at + 512;
        let end = start.checked_add(size).filter(|end| *end <= archive.len());
        let Some(end) = end else {
            return fail("a file runs past the end of the archive");
        };
        if (kind == b'0' || kind == 0) && files.len() < MAX_TENSORS {
            files.insert(name, &archive[start..end]);
        }
        at = start + size.div_ceil(512) * 512;
    }
    Ok(files)
}

/// The keys and values of a table, the values borrowed from it.
type Entries<'a> = Vec<(Vec<u8>, &'a [u8])>;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }

    fn varint(&mut self) -> Result<u64, BundleError> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let Some(&byte) = self.bytes.get(self.at) else {
                return fail("a number is cut");
            };
            self.at += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        fail("a number is too long")
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], BundleError> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len());
        let Some(end) = end else {
            return fail("a field runs past its block");
        };
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }
}

/// The key and value pairs of a table block, with its restart array cut off.
fn block_entries(block: &[u8]) -> Result<Entries<'_>, BundleError> {
    if block.len() < 4 {
        return fail("a block is too short");
    }
    let restarts = u32::from_le_bytes([
        block[block.len() - 4],
        block[block.len() - 3],
        block[block.len() - 2],
        block[block.len() - 1],
    ]) as usize;
    let data_end = restarts
        .checked_mul(4)
        .and_then(|size| block.len().checked_sub(4 + size))
        .ok_or(BundleError("a block has too many restarts"))?;
    let mut reader = Reader::new(&block[..data_end]);
    let mut key = Vec::new();
    let mut entries = Vec::new();
    while !reader.done() {
        let shared = reader.varint()? as usize;
        let own = reader.varint()? as usize;
        let value_len = reader.varint()? as usize;
        if shared > key.len() || entries.len() >= MAX_TENSORS * 4 {
            return fail("a key does not follow the one before");
        }
        key.truncate(shared);
        key.extend_from_slice(reader.take(own)?);
        entries.push((key.clone(), reader.take(value_len)?));
    }
    Ok(entries)
}

fn handle(value: &[u8]) -> Result<(usize, usize), BundleError> {
    let mut reader = Reader::new(value);
    Ok((reader.varint()? as usize, reader.varint()? as usize))
}

/// A block of the table at its handle, without its trailer. Compressed blocks are refused:
/// the index of a checkpoint is written without compression.
fn block(table: &[u8], (offset, size): (usize, usize)) -> Result<&[u8], BundleError> {
    let end = offset
        .checked_add(size)
        .and_then(|end| end.checked_add(5))
        .filter(|end| *end <= table.len());
    let Some(end) = end else {
        return fail("a block runs past the end of the index");
    };
    if table[end - 5] != 0 {
        return fail("a block of the index is compressed");
    }
    Ok(&table[offset..offset + size])
}

/// The entries of a table of LevelDB: the index block names the data blocks.
fn table(bytes: &[u8]) -> Result<Entries<'_>, BundleError> {
    const MAGIC: u64 = 0xdb47_7524_8b80_fb57;
    if bytes.len() < 48 {
        return fail("the index is too short");
    }
    let footer = &bytes[bytes.len() - 48..];
    let magic = u64::from_le_bytes([
        footer[40], footer[41], footer[42], footer[43], footer[44], footer[45], footer[46],
        footer[47],
    ]);
    if magic != MAGIC {
        return fail("the index is not a table");
    }
    let mut reader = Reader::new(footer);
    let _metaindex = (reader.varint()?, reader.varint()?);
    let index = (reader.varint()? as usize, reader.varint()? as usize);
    let mut entries = Vec::new();
    for (_, value) in block_entries(block(bytes, index)?)? {
        entries.extend(block_entries(block(bytes, handle(value)?)?)?);
    }
    Ok(entries)
}

/// Where the values of a tensor of 32-bit floats are, and its dimensions.
struct Entry {
    dims: Vec<usize>,
    offset: usize,
    size: usize,
    float: bool,
}

fn shape(bytes: &[u8]) -> Result<Vec<usize>, BundleError> {
    let mut reader = Reader::new(bytes);
    let mut dims = Vec::new();
    while !reader.done() {
        let tag = reader.varint()?;
        match (tag >> 3, tag & 7) {
            (2, 2) => {
                let len = reader.varint()? as usize;
                let mut dim = Reader::new(reader.take(len)?);
                let mut size = 0;
                while !dim.done() {
                    let tag = dim.varint()?;
                    match (tag >> 3, tag & 7) {
                        (1, 0) => size = dim.varint()? as usize,
                        (_, 0) => {
                            dim.varint()?;
                        }
                        (_, 2) => {
                            let skip = dim.varint()? as usize;
                            dim.take(skip)?;
                        }
                        _ => return fail("a dimension has an unknown field"),
                    }
                }
                dims.push(size);
            }
            (_, 0) => {
                reader.varint()?;
            }
            (_, 2) => {
                let skip = reader.varint()? as usize;
                reader.take(skip)?;
            }
            _ => return fail("a shape has an unknown field"),
        }
    }
    Ok(dims)
}

fn entry(bytes: &[u8]) -> Result<Entry, BundleError> {
    let mut reader = Reader::new(bytes);
    let mut found = Entry {
        dims: Vec::new(),
        offset: 0,
        size: 0,
        float: false,
    };
    while !reader.done() {
        let tag = reader.varint()?;
        match (tag >> 3, tag & 7) {
            // DT_FLOAT is 1.
            (1, 0) => found.float = reader.varint()? == 1,
            (2, 2) => {
                let len = reader.varint()? as usize;
                found.dims = shape(reader.take(len)?)?;
            }
            (4, 0) => found.offset = reader.varint()? as usize,
            (5, 0) => found.size = reader.varint()? as usize,
            (_, 0) => {
                reader.varint()?;
            }
            (_, 2) => {
                let skip = reader.varint()? as usize;
                reader.take(skip)?;
            }
            (_, 5) => {
                reader.take(4)?;
            }
            (_, 1) => {
                reader.take(8)?;
            }
            _ => return fail("an entry has an unknown field"),
        }
    }
    Ok(found)
}

/// The tensors of 32-bit floats of a checkpoint whose name passes `wanted`, from its index and
/// its single data file.
pub fn checkpoint(
    index: &[u8],
    data: &[u8],
    wanted: impl Fn(&str) -> bool,
) -> Result<Tensors, BundleError> {
    let mut tensors = Tensors::new();
    let mut total = 0usize;
    for (key, value) in table(index)? {
        let Ok(name) = std::str::from_utf8(&key) else {
            continue;
        };
        if name.is_empty() || !wanted(name) {
            continue;
        }
        let found = entry(value)?;
        if !found.float {
            continue;
        }
        let count = found
            .dims
            .iter()
            .try_fold(1usize, |count, dim| count.checked_mul(*dim))
            .filter(|count| count.checked_mul(4) == Some(found.size))
            .ok_or(BundleError(
                "a tensor has a size that does not fit its shape",
            ))?;
        total = total.saturating_add(count);
        if total > MAX_VALUES || tensors.len() >= MAX_TENSORS {
            return fail("the model is larger than any Platter takes");
        }
        let end = found
            .offset
            .checked_add(found.size)
            .filter(|end| *end <= data.len())
            .ok_or(BundleError("a tensor runs past the end of the data"))?;
        let values = data[found.offset..end]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| f32::from_le_bytes(*bytes))
            .collect();
        tensors.insert(
            name.to_owned(),
            Tensor {
                dims: found.dims,
                values,
            },
        );
    }
    Ok(tensors)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn varint(mut value: u64, out: &mut Vec<u8>) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    /// A block of whole keys, one restart.
    fn block_of(entries: &[(&[u8], Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (key, value) in entries {
            varint(0, &mut out);
            varint(key.len() as u64, &mut out);
            varint(value.len() as u64, &mut out);
            out.extend_from_slice(key);
            out.extend_from_slice(value);
        }
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out
    }

    /// The index and the data of a checkpoint holding `tensors`, as TensorFlow lays them out.
    pub(crate) fn checkpoint_of(tensors: &[(&str, Vec<usize>, Vec<f32>)]) -> (Vec<u8>, Vec<u8>) {
        let mut data = Vec::new();
        let mut entries: Vec<(&[u8], Vec<u8>)> = vec![(b"", vec![0x08, 0x01])];
        let mut sorted: Vec<_> = tensors.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        for (name, dims, values) in sorted {
            let mut proto = vec![0x08, 0x01];
            let mut shape = Vec::new();
            for dim in dims {
                let mut field = Vec::new();
                field.push(0x08);
                varint(*dim as u64, &mut field);
                shape.push(0x12);
                varint(field.len() as u64, &mut shape);
                shape.extend(field);
            }
            proto.push(0x12);
            varint(shape.len() as u64, &mut proto);
            proto.extend(shape);
            proto.push(0x20);
            varint(data.len() as u64, &mut proto);
            proto.push(0x28);
            varint(values.len() as u64 * 4, &mut proto);
            // A checksum, which the reader passes over.
            proto.push(0x35);
            proto.extend_from_slice(&[1, 2, 3, 4]);
            for value in values {
                data.extend_from_slice(&value.to_le_bytes());
            }
            entries.push((name.as_bytes(), proto));
        }
        let mut index = block_of(&entries);
        let data_len = index.len();
        index.extend_from_slice(&[0, 0, 0, 0, 0]);
        let mut handle = Vec::new();
        varint(0, &mut handle);
        varint(data_len as u64, &mut handle);
        let index_offset = index.len();
        let index_block = block_of(&[(b"~", handle)]);
        let index_len = index_block.len();
        index.extend(index_block);
        index.extend_from_slice(&[0, 0, 0, 0, 0]);
        let mut footer = Vec::new();
        varint(0, &mut footer);
        varint(0, &mut footer);
        varint(index_offset as u64, &mut footer);
        varint(index_len as u64, &mut footer);
        footer.resize(40, 0);
        footer.extend_from_slice(&0xdb47_7524_8b80_fb57u64.to_le_bytes());
        index.extend(footer);
        (index, data)
    }

    /// A tar archive of `files`.
    pub(crate) fn tar_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, content) in files {
            let mut header = [0u8; 512];
            header[..name.len()].copy_from_slice(name.as_bytes());
            let size = format!("{:011o}\0", content.len());
            header[124..136].copy_from_slice(size.as_bytes());
            header[156] = b'0';
            out.extend_from_slice(&header);
            out.extend_from_slice(content);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.extend_from_slice(&[0u8; 1024]);
        out
    }

    #[test]
    fn a_checkpoint_gives_its_float_tensors_by_name() {
        let (index, data) = checkpoint_of(&[
            (
                "conv2d/kernel",
                vec![2, 3],
                vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            ),
            ("conv2d/bias", vec![3], vec![0.5, 0.25, 0.125]),
            ("global_step", vec![], vec![9.0]),
        ]);
        let tensors = checkpoint(&index, &data, |name| name.starts_with("conv2d")).unwrap();
        assert_eq!(tensors.len(), 2);
        assert_eq!(tensors["conv2d/kernel"].dims, vec![2, 3]);
        assert_eq!(tensors["conv2d/bias"].values, vec![0.5, 0.25, 0.125]);
        // Data cut short is refused.
        assert!(checkpoint(&index, &data[..8], |_| true).is_err());
        assert!(checkpoint(&index[..index.len() - 1], &data, |_| true).is_err());
    }

    #[test]
    fn a_tar_gives_its_files_and_refuses_one_cut_short() {
        let archive = tar_of(&[
            ("4stems/model.index", b"index"),
            ("4stems/checkpoint", b"x"),
        ]);
        let files = untar(&archive).unwrap();
        assert_eq!(
            files.get("4stems/model.index").copied(),
            Some(&b"index"[..])
        );
        assert_eq!(files.len(), 2);
        assert!(untar(&archive[..515]).is_err());
    }
}
