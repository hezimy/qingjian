//! 搜狗词库：拼音备份 `.bin`（文件头 `SGPU`）和细胞词库 `.scel`。
//!
//! 备份按文件头里的索引表逐条读：词频、音节下标、UTF-16 词。下标表与现行搜狗备份一致。
//! 细胞词库自带拼音表，词条区在表后面。旧版加密备份（没有 `SGPU`）不解析。

use std::collections::HashMap;
use std::path::Path;

use crate::error::DictionaryError;

const SGPU: u32 = u32::from_le_bytes(*b"SGPU");

/// 转成青简 TSV，并给一个显示名。不是搜狗文件就返回 `Ok(None)`。
pub(super) fn try_convert(source: &Path) -> Result<Option<(String, String)>, DictionaryError> {
    let kind = kind_of(source)?;
    let Some(kind) = kind else {
        return Ok(None);
    };
    let bytes = std::fs::read(source)?;
    let entries = match kind {
        Kind::Backup => backup_entries(&bytes)?,
        Kind::Scel => scel_entries(&bytes)?,
    };
    if entries.is_empty() {
        return Err(DictionaryError::Corrupt(
            "no usable entries in this sogou dictionary",
        ));
    }
    let name = match kind {
        Kind::Backup => "搜狗词库".to_owned(),
        Kind::Scel => scel_name(&bytes).unwrap_or_else(|| "搜狗细胞词库".to_owned()),
    };
    Ok(Some((to_tsv(entries), name)))
}

enum Kind {
    Backup,
    Scel,
}

fn kind_of(source: &Path) -> Result<Option<Kind>, DictionaryError> {
    let ext = source
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "scel" {
        return Ok(Some(Kind::Scel));
    }
    let mut magic = [0u8; 4];
    let file = std::fs::File::open(source)?;
    use std::io::Read;
    let mut file = file;
    let n = file.read(&mut magic)?;
    if n == 4 && u32::from_le_bytes(magic) == SGPU {
        return Ok(Some(Kind::Backup));
    }
    if ext == "bin" {
        return Err(DictionaryError::Corrupt(
            "unsupported sogou backup; export the lexicon again from current sogou pinyin",
        ));
    }
    Ok(None)
}

fn backup_entries(bytes: &[u8]) -> Result<Vec<Entry>, DictionaryError> {
    if bytes.len() < 0x50 || u32::from_le_bytes(bytes[0..4].try_into().unwrap()) != SGPU {
        return Err(DictionaryError::Corrupt(
            "unsupported sogou backup; export the lexicon again from current sogou pinyin",
        ));
    }
    let index_at = read_u32(bytes, 0x38)? as usize;
    let count = read_u32(bytes, 0x40)? as usize;
    let dict_at = read_u32(bytes, 0x44)? as usize;
    if count > 2_000_000 || index_at.saturating_add(count.saturating_mul(4)) > bytes.len() {
        return Err(DictionaryError::Corrupt(
            "sogou backup index is out of range",
        ));
    }
    let mut entries = Vec::new();
    for index in 0..count {
        let at = index_at + index * 4;
        let offset = read_u32(bytes, at)? as usize;
        let Some(entry) = backup_entry(bytes, dict_at.saturating_add(offset)) else {
            continue;
        };
        entries.push(entry);
    }
    Ok(entries)
}

/// 一条词：`freq`、跳过 7 字节、音节下标、再跳过 2 字节、UTF-16 词。
fn backup_entry(bytes: &[u8], mut at: usize) -> Option<Entry> {
    let frequency = read_u16(bytes, at).ok()?;
    at = at.checked_add(2 + 7)?;
    let py_bytes = read_u16(bytes, at).ok()? as usize;
    at = at.checked_add(2)?;
    if py_bytes == 0 || !py_bytes.is_multiple_of(2) || at + py_bytes > bytes.len() {
        return None;
    }
    let mut pinyin = Vec::with_capacity(py_bytes / 2);
    for syllable in bytes[at..at + py_bytes].chunks_exact(2) {
        let index = u16::from_le_bytes([syllable[0], syllable[1]]) as usize;
        pinyin.push((*PINYIN.get(index)?).to_owned());
    }
    at += py_bytes + 2;
    let word_bytes = read_u16(bytes, at).ok()? as usize;
    at = at.checked_add(2)?;
    if word_bytes == 0 || !word_bytes.is_multiple_of(2) || at + word_bytes > bytes.len() {
        return None;
    }
    let word = utf16(&bytes[at..at + word_bytes])?;
    Some(Entry {
        word,
        pinyin,
        frequency: u32::from(frequency).max(1),
    })
}

fn scel_entries(bytes: &[u8]) -> Result<Vec<Entry>, DictionaryError> {
    if bytes.len() < 0x1544 {
        return Err(DictionaryError::Corrupt("sogou scel file is too small"));
    }
    let groups = read_u32(bytes, 0x120)? as usize;
    let py_count = read_u32(bytes, 0x1540)? as usize;
    if groups > 2_000_000 || !(1..=4096).contains(&py_count) {
        return Err(DictionaryError::Corrupt(
            "sogou scel header is out of range",
        ));
    }
    let mut at = 0x1544;
    let mut table = vec![String::new(); py_count];
    for _ in 0..py_count {
        let index = read_u16(bytes, at)
            .map_err(|_| DictionaryError::Corrupt("sogou scel pinyin table is truncated"))?
            as usize;
        let size = read_u16(bytes, at + 2)? as usize;
        at += 4;
        if index >= table.len() || at + size > bytes.len() {
            return Err(DictionaryError::Corrupt(
                "sogou scel pinyin table is truncated",
            ));
        }
        table[index] = utf16(&bytes[at..at + size]).unwrap_or_default();
        at += size;
    }
    let mut entries = Vec::new();
    for _ in 0..groups {
        let Some(next) = scel_group(bytes, at, &table, &mut entries) else {
            break;
        };
        at = next;
    }
    Ok(entries)
}

/// 一组同音词。读失败就停，已经收下的留下。
fn scel_group(
    bytes: &[u8],
    mut at: usize,
    table: &[String],
    out: &mut Vec<Entry>,
) -> Option<usize> {
    let same = read_u16(bytes, at).ok()? as usize;
    let py_bytes = read_u16(bytes, at + 2).ok()? as usize;
    at += 4;
    if !py_bytes.is_multiple_of(2) || at + py_bytes > bytes.len() || same > 10_000 {
        return None;
    }
    let mut pinyin = Vec::with_capacity(py_bytes / 2);
    for syllable in bytes[at..at + py_bytes].chunks_exact(2) {
        let index = u16::from_le_bytes([syllable[0], syllable[1]]) as usize;
        let text = table.get(index).filter(|s| !s.is_empty())?;
        pinyin.push(text.clone());
    }
    at += py_bytes;
    for _ in 0..same {
        let word_bytes = read_u16(bytes, at).ok()? as usize;
        at = at.checked_add(2)?;
        if word_bytes == 0 || at + word_bytes + 12 > bytes.len() {
            return None;
        }
        if let Some(word) = utf16(&bytes[at..at + word_bytes]) {
            out.push(Entry {
                word,
                pinyin: pinyin.clone(),
                frequency: 1,
            });
        }
        // 词后 2+4 字节的附加信息，再跳过 6 字节。
        at = at.checked_add(word_bytes + 12)?;
    }
    Some(at)
}

fn scel_name(bytes: &[u8]) -> Option<String> {
    let at = 0x130;
    let end = at + 64;
    if bytes.len() < end {
        return None;
    }
    let raw = &bytes[at..end];
    let end = raw.chunks_exact(2).position(|c| c == [0, 0])? * 2;
    let name = utf16(&raw[..end])?;
    (name.chars().count() <= 32 && name.chars().all(|c| !c.is_control())).then_some(name)
}

struct Entry {
    word: String,
    pinyin: Vec<String>,
    frequency: u32,
}

fn to_tsv(entries: Vec<Entry>) -> String {
    let mut best: HashMap<(String, String), u32> = HashMap::new();
    for entry in entries {
        if entry.pinyin.is_empty() || entry.word.contains(['\t', '\n', '\r']) {
            continue;
        }
        let key = (entry.word, entry.pinyin.join(" "));
        best.entry(key)
            .and_modify(|frequency| *frequency = (*frequency).max(entry.frequency))
            .or_insert(entry.frequency);
    }
    let mut tsv = String::new();
    for ((word, pinyin), frequency) in best {
        tsv.push_str(&word);
        tsv.push('\t');
        tsv.push_str(&pinyin);
        tsv.push('\t');
        tsv.push_str(&frequency.to_string());
        tsv.push('\n');
    }
    tsv
}

fn utf16(bytes: &[u8]) -> Option<String> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&units)
        .ok()
        .filter(|text| !text.is_empty())
}

fn read_u16(bytes: &[u8], at: usize) -> Result<u16, DictionaryError> {
    let chunk = bytes
        .get(at..at + 2)
        .ok_or(DictionaryError::Corrupt("sogou dictionary is truncated"))?;
    Ok(u16::from_le_bytes([chunk[0], chunk[1]]))
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32, DictionaryError> {
    let chunk = bytes
        .get(at..at + 4)
        .ok_or(DictionaryError::Corrupt("sogou dictionary is truncated"))?;
    Ok(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
}

include!("pinyin_table.rs");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_one_sogou_backup_entry() {
        let mut bytes = vec![0u8; 0x90];
        bytes[0..4].copy_from_slice(b"SGPU");
        write_u32(&mut bytes, 0x38, 0x80);
        write_u32(&mut bytes, 0x40, 1);
        write_u32(&mut bytes, 0x44, 0x84);
        // 索引指向词条区起点。
        bytes.resize(0xA0, 0);
        write_u32(&mut bytes, 0x80, 0);
        let mut at = 0x84;
        write_u16_at(&mut bytes, &mut at, 320);
        at += 7;
        write_u16_at(&mut bytes, &mut at, 4);
        write_u16_at(&mut bytes, &mut at, 0);
        write_u16_at(&mut bytes, &mut at, 1);
        at += 2;
        write_u16_at(&mut bytes, &mut at, 4);
        // 啊啊
        bytes[at..at + 4].copy_from_slice(&[0x4A, 0x55, 0x4A, 0x55]);
        let entries = backup_entries(&bytes).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].word, "啊啊");
        assert_eq!(entries[0].pinyin, ["a", "ai"]);
        assert_eq!(entries[0].frequency, 320);
    }

    fn write_u32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u16_at(bytes: &mut [u8], at: &mut usize, value: u16) {
        bytes[*at..*at + 2].copy_from_slice(&value.to_le_bytes());
        *at += 2;
    }
}
