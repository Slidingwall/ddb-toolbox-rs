use anyhow::Result;
use memchr::memmem;
use serde_yaml::Value;
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;
pub type ArtpType = BTreeMap<String, Value>;
pub type ArtuType = BTreeMap<String, Value>;
pub type ArtType = BTreeMap<String, Value>;
fn read_u32_le<R: Read>(cur: &mut R) -> Result<u32> {
    let mut buf = [0u8; 4];
    cur.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}
fn read_f32_le<R: Read>(cur: &mut R) -> Result<f32> {
    let mut buf = [0u8; 4];
    cur.read_exact(&mut buf)?;
    Ok(f32::from_le_bytes(buf))
}
fn read_f64_le<R: Read>(cur: &mut R) -> Result<f64> {
    let mut buf = [0u8; 8];
    cur.read_exact(&mut buf)?;
    Ok(f64::from_le_bytes(buf))
}
fn read_u64_le<R: Read>(cur: &mut R) -> Result<u64> {
    let mut buf = [0u8; 8];
    cur.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}
fn read_u16_le<R: Read>(cur: &mut R) -> Result<u16> {
    let mut buf = [0u8; 2];
    cur.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
}
pub fn read_str<R: Read>(cur: &mut R) -> Result<String> {
    let len = read_u32_le(cur)? as usize;
    let mut buf = vec![0u8; len];
    cur.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}
pub fn str_to_data(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(4 + s.len());
    v.extend_from_slice(&(s.len() as u32).to_le_bytes());
    v.extend_from_slice(s.as_bytes());
    v
}
pub fn read_arr<R: Read>(cur: &mut R) -> Result<Vec<u8>> {
    let mut sig = [0u8; 4];
    cur.read_exact(&mut sig)?;
    let mut _tmp = [0u8; 4];
    cur.read_exact(&mut _tmp)?;
    let mut _tmp8 = [0u8; 8];
    cur.read_exact(&mut _tmp8)?;
    let mut out = [0u8; 4];
    cur.read_exact(&mut out)?;
    Ok(out.to_vec())
}
pub fn reverse_search(data: &[u8], pat: &[u8], offset: usize, limit: i32) -> usize {
    let limit = if limit == -1 {
        offset - pat.len()
    } else {
        limit as usize
    };
    let mut pos = offset - pat.len();
    while pos > 0 {
        if pos + pat.len() <= data.len() && &data[pos..pos+pat.len()] == pat {
            return pos;
        }
        if offset - pos > limit {
            break;
        }
        pos -= 1;
    }
    -1isize as usize
}
pub fn stream_reverse_search<T: Read + Seek>(
    cur: &mut T,
    pat: &[u8],
    offset: usize,
    limit: i32,
) -> usize {
    let limit = if limit == -1 {
        10 * 1024 * 1024
    } else {
        limit as usize
    };
    let mut pos = offset - pat.len();
    let mut buf = vec![0u8; pat.len()];
    while pos > 0 {
        let _ = cur.seek(SeekFrom::Start(pos as u64));
        if cur.read_exact(&mut buf).is_ok() && buf == pat {
            return pos;
        }
        if offset - pos > limit {
            break;
        }
        pos -= 1;
    }
    -1isize as usize
}
#[derive(Debug, Default)]
pub struct DDIModel {
    pub ddi_bytes: Vec<u8>,
    pub phdc_data: BTreeMap<String, Value>,
    pub tdb_data: BTreeMap<u32, String>,
    pub sta_data: BTreeMap<u32, ArtuType>,
    pub art_data: BTreeMap<u32, ArtType>,
    pub vqm_data: Option<BTreeMap<u32, ArtpType>>,
    pub offset_map: BTreeMap<String, (usize, usize)>,
    pub ddi_data_dict: BTreeMap<String, Value>,
}
impl DDIModel {
    pub fn new(ddi_bytes: Vec<u8>) -> Self {
        Self { ddi_bytes, ..Default::default() }
    }
    pub fn read(&mut self, _temp_path: Option<&Path>, cat_only: bool) -> Result<()> {
        if cat_only {
            return Ok(());
        }
        let mut cur = Cursor::new(&self.ddi_bytes[..]);
        if let Some(pos) = memmem::find(&self.ddi_bytes, b"PHDC") {
            cur.seek(SeekFrom::Start(pos as u64))?;
            self.phdc_data = Self::read_phdc(&mut cur)?;
            self.offset_map.insert("phdc".into(), (pos, cur.stream_position()? as usize));
        }
        let tdb_sig = [0xffu8; 8].into_iter().chain(b"TDB ".iter().copied()).collect::<Vec<_>>();
        if let Some(pos) = memmem::find(&self.ddi_bytes, &tdb_sig) {
            cur.seek(SeekFrom::Start(pos as u64))?;
            self.tdb_data = Self::read_tdb(&mut cur)?;
            self.offset_map.insert("tdb".into(), (pos, cur.stream_position()? as usize));
        }
        let dbv_sig = [0x00u8; 8].into_iter().chain(b"DBV ".iter().copied()).collect::<Vec<_>>();
        if let Some(pos) = memmem::find(&self.ddi_bytes, &dbv_sig) {
            cur.seek(SeekFrom::Start(pos as u64))?;
            Self::read_dbv(&mut cur)?;
            self.offset_map.insert("dbv".into(), (pos, cur.stream_position()? as usize));
        }
        let sta_sig = [0x00u8; 8].into_iter().chain(b"STA ".iter().copied()).collect::<Vec<_>>();
        if let Some(pos) = memmem::find(&self.ddi_bytes, &sta_sig) {
            let arr_pos = reverse_search(&self.ddi_bytes, b"ARR ", pos, -1);
            let sta_offset = arr_pos - 8;
            cur.seek(SeekFrom::Start(sta_offset as u64))?;
            self.sta_data = Self::read_sta(&mut cur)?;
            self.offset_map.insert("sta".into(), (sta_offset, cur.stream_position()? as usize));
        }
        let art_sig = [0x00u8; 8].into_iter().chain(b"ART ".iter().copied()).collect::<Vec<_>>();
        if let Some(pos) = memmem::find(&self.ddi_bytes, &art_sig) {
            let arr_pos = reverse_search(&self.ddi_bytes, b"ARR ", pos, -1);
            let art_offset = arr_pos - 8;
            cur.seek(SeekFrom::Start(art_offset as u64))?;
            self.art_data = self.read_art(&mut cur)?;
            self.offset_map.insert("art".into(), (art_offset, cur.stream_position()? as usize));
        }
        let vqm_sig = [0xffu8; 8].into_iter().chain(b"VQM ".iter().copied()).collect::<Vec<_>>();
        if let Some(pos) = memmem::find(&self.ddi_bytes, &vqm_sig) {
            cur.seek(SeekFrom::Start(pos as u64))?;
            self.vqm_data = Some(Self::read_vqm(&mut cur)?);
            self.offset_map.insert("vqm".into(), (pos, cur.stream_position()? as usize));
        }
        self.build_ddi_dict();
        Ok(())
    }
    fn build_ddi_dict(&mut self) {
        let mut ddi_dict = BTreeMap::new();
        let mut sta_dict = BTreeMap::new();
        for stau in self.sta_data.values() {
            let phoneme = stau["phoneme"].as_str().unwrap_or_default();
            let mut items = Vec::new();
            if let Some(stap) = stau.get("stap").and_then(|v| v.as_mapping()) {
                for stap_item in stap.values() {
                    let mut item = BTreeMap::new();
                    item.insert("snd".into(), stap_item.get("snd").cloned().unwrap_or(Value::Null));
                    item.insert("epr".into(), stap_item.get("epr").cloned().unwrap_or(Value::Null));
                    item.insert("pitch".into(), stap_item.get("pitch1").cloned().unwrap_or(Value::Null));
                    items.push(Value::Mapping(item.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
                }
            }
            sta_dict.insert(phoneme.to_string(), Value::Sequence(items));
        }
        ddi_dict.insert("sta".to_string(), Value::Mapping(sta_dict.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
        let mut art_dict = BTreeMap::new();
        for art in self.art_data.values() {
            if let Some(artu) = art.get("artu").and_then(|v| v.as_mapping()) {
                for au in artu.values() {
                    let key = format!("{} {}", art["phoneme"].as_str().unwrap_or_default(), au["phoneme"].as_str().unwrap_or_default());
                    let mut items = Vec::new();
                    if let Some(artp) = au.get("artp").and_then(|v| v.as_mapping()) {
                        for artp_item in artp.values() {
                            let mut item = BTreeMap::new();
                            item.insert("snd".into(), artp_item.get("snd").cloned().unwrap_or(Value::Null));
                            item.insert("snd_start".into(), artp_item.get("snd_start").cloned().unwrap_or(Value::Null));
                            item.insert("epr".into(), artp_item.get("epr").cloned().unwrap_or(Value::Null));
                            item.insert("pitch".into(), artp_item.get("pitch1").cloned().unwrap_or(Value::Null));
                            items.push(Value::Mapping(item.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
                        }
                    }
                    art_dict.insert(key, Value::Sequence(items));
                }
            }
            if let Some(sub_art) = art.get("art").and_then(|v| v.as_mapping()) {
                for sub in sub_art.values() {
                    if let Some(artu) = sub.get("artu").and_then(|v| v.as_mapping()) {
                        for au in artu.values() {
                            let key = format!("{} {} {}", art["phoneme"].as_str().unwrap_or_default(), sub["phoneme"].as_str().unwrap_or_default(), au["phoneme"].as_str().unwrap_or_default());
                            let mut items = Vec::new();
                            if let Some(artp) = au.get("artp").and_then(|v| v.as_mapping()) {
                                for artp_item in artp.values() {
                                    let mut item = BTreeMap::new();
                                    item.insert("snd".into(), artp_item.get("snd").cloned().unwrap_or(Value::Null));
                                    item.insert("snd_start".into(), artp_item.get("snd_start").cloned().unwrap_or(Value::Null));
                                    item.insert("epr".into(), artp_item.get("epr").cloned().unwrap_or(Value::Null));
                                    item.insert("pitch".into(), artp_item.get("pitch1").cloned().unwrap_or(Value::Null));
                                    items.push(Value::Mapping(item.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
                                }
                            }
                            art_dict.insert(key, Value::Sequence(items));
                        }
                    }
                }
            }
        }
        ddi_dict.insert("art".to_string(), Value::Mapping(art_dict.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
        if let Some(vqm) = &self.vqm_data {
            let mut vqm_items = Vec::new();
            for v in vqm.values() {
                let mut item = BTreeMap::new();
                item.insert("snd".into(), v.get("snd").cloned().unwrap_or(Value::Null));
                item.insert("epr".into(), v.get("epr").cloned().unwrap_or(Value::Null));
                item.insert("pitch".into(), v.get("pitch1").cloned().unwrap_or(Value::Null));
                vqm_items.push(Value::Mapping(item.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
            }
            ddi_dict.insert("vqm".to_string(), Value::Sequence(vqm_items));
        }
        self.ddi_data_dict = ddi_dict;
    }
    fn read_phdc<R: Read + Seek>(cur: &mut R) -> Result<BTreeMap<String, Value>> {
        let mut phdc = BTreeMap::new();
        let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
        let phdc_size = read_u32_le(cur)?;
        let _ = read_u32_le(cur)?;
        let phoneme_num = read_u32_le(cur)?;
        let (mut voiced, mut unvoiced) = (Vec::new(), Vec::new());
        for _ in 0..phoneme_num {
            let mut buf = [0u8; 0x1F]; cur.read_exact(&mut buf)?;
            let s = String::from_utf8_lossy(&buf[..0x1E]).trim_matches('\0').to_string();
            if buf[0x1E] == 0 { voiced.push(Value::String(s)) } else { unvoiced.push(Value::String(s)) }
        }
        let mut phoneme_map = BTreeMap::new();
        phoneme_map.insert("voiced".into(), Value::Sequence(voiced));
        phoneme_map.insert("unvoiced".into(), Value::Sequence(unvoiced));
        phdc.insert("phoneme".into(), Value::Mapping(phoneme_map.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
        let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
        let phg2_size = read_u32_le(cur)?;
        let phg2_num = read_u32_le(cur)?;
        let mut phg2_data = BTreeMap::new();
        for _ in 0..phg2_num {
            let key = read_str(cur)?;
            let temp_num = read_u32_le(cur)?;
            let mut inner = BTreeMap::new();
            for _ in 0..temp_num {
                let idx = read_u32_le(cur)?;
                let val = read_str(cur)?;
                inner.insert(idx.to_string(), Value::String(val));
            }
            let _ = read_u32_le(cur)?;
            phg2_data.insert(key, Value::Mapping(inner.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
        }
        phdc.insert("phg2".into(), Value::Mapping(phg2_data.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
        let epr_guide_num = read_u32_le(cur)?;
        let base = phdc_size - phg2_size - 0x10 - 0x1F * phoneme_num as u32 - 4;
        let mut epr_guide_bytes = vec![0u8; base as usize];
        cur.read_exact(&mut epr_guide_bytes)?;
        let mut offset = 0;
        let mut epr_guide_data = BTreeMap::new();
        for _ in 0..epr_guide_num {
            let key = String::from_utf8_lossy(&epr_guide_bytes[offset..offset+0x20]).trim_matches('\0').to_string();
            offset += 0x20;
            let _ = u32::from_le_bytes(epr_guide_bytes[offset..offset+4].try_into()?);
            offset += 4;
            let mut epr_list = Vec::new();
            while offset < epr_guide_bytes.len() && epr_guide_bytes[offset] == 0 {
                let slice = &epr_guide_bytes[offset..offset+7];
                let pos = slice.iter().position(|&b| b != 0).unwrap_or(0);
                let val = slice[pos..].iter().map(|b| format!("{:02x}", b)).collect::<String>();
                epr_list.push(Value::String(val));
                offset += 8;
            }
            epr_guide_data.insert(key, Value::Sequence(epr_list));
        }
        phdc.insert("epr_guide".into(), Value::Mapping(epr_guide_data.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
        Ok(phdc)
    }
    fn read_tdb<R: Read + Seek>(cur: &mut R) -> Result<BTreeMap<u32, String>> {
        let mut tdb = BTreeMap::new();
        let mut _buf8 = [0u8; 8]; cur.read_exact(&mut _buf8)?;
        let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
        let _ = read_u32_le(cur)?;
        let _ = read_u64_le(cur)?;
        let num = read_u32_le(cur)?;
        for _ in 0..num {
            let mut _buf8 = [0u8; 8]; cur.read_exact(&mut _buf8)?;
            let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
            let _ = read_u32_le(cur)?;
            let _ = read_u64_le(cur)?;
            let idx = read_u32_le(cur)?;
            let sn = read_u32_le(cur)?;
            for _ in 0..sn {
                let mut _buf8 = [0u8; 8]; cur.read_exact(&mut _buf8)?;
                let _ = read_arr(cur)?;
                let _ = read_str(cur)?;
            }
            tdb.insert(idx, read_str(cur)?);
        }
        let _ = read_str(cur)?;
        Ok(tdb)
    }
    fn read_dbv<R: Read + Seek>(cur: &mut R) -> Result<()> {
        let _ = read_u64_le(cur)?;
        let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
        let _ = read_u32_le(cur)?;
        let _ = read_u64_le(cur)?;
        let _ = read_u32_le(cur)?;
        Ok(())
    }
    fn read_sta<R: Read + Seek>(cur: &mut R) -> Result<BTreeMap<u32, ArtuType>> {
        let mut sta = BTreeMap::new();
        let _ = read_u64_le(cur)?;
        let _ = read_arr(cur)?;
        let _ = read_u64_le(cur)?;
        let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
        let _ = read_u32_le(cur)?;
        let _ = read_u64_le(cur)?;
        let num = read_u32_le(cur)?;
        for _ in 0..num {
            let _ = read_u64_le(cur)?;
            let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
            let _ = read_u32_le(cur)?;
            let _ = read_u32_le(cur)?;
            let _ = read_u32_le(cur)?;
            let idx = read_u32_le(cur)?;
            let mut _buf8 = [0u8; 8]; cur.read_exact(&mut _buf8)?;
            let n_stap = read_u32_le(cur)?;
            let mut stap = BTreeMap::new();
            for _ in 0..n_stap {
                let _ = read_u64_le(cur)?;
                let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
                let _ = read_u32_le(cur)?;
                let _ = read_u32_le(cur)?;
                let _ = read_u32_le(cur)?;
                let duration = read_f64_le(cur)?;
                let _ = read_u16_le(cur)?;
                let pitch1 = read_f32_le(cur)?;
                let pitch2 = read_f32_le(cur)?;
                let unknown2 = read_f32_le(cur)?;
                let dynamics = read_f32_le(cur)?;
                let tempo = read_f32_le(cur)?;
                let _ = read_u32_le(cur)?;
                let _ = read_u32_le(cur)?;
                let _ = read_u64_le(cur)?;
                let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
                let _ = read_u32_le(cur)?;
                let _ = read_str(cur)?;
                let snd_length = read_u32_le(cur)?;
                let _ = read_u32_le(cur)?;
                let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
                let _ = read_u32_le(cur)?;
                let _ = read_str(cur)?;
                let mut _buf4 = [0u8; 4]; cur.read_exact(&mut _buf4)?;
                let epr_num = read_u32_le(cur)?;
                let mut epr = Vec::new();
                for _ in 0..epr_num {
                    let pos = cur.stream_position()?;
                    let off = read_u64_le(cur)?;
                    epr.push(Value::String(format!("{pos:08x}={off:08x}")));
                }
                let fs = read_u32_le(cur)?;
                let _ = read_u16_le(cur)?;
                let snd_id = read_u32_le(cur)?;
                let snd_pos = cur.stream_position()?;
                let snd_off = read_u64_le(cur)?;
                let snd = Value::String(format!("{snd_pos:08x}={snd_off:016x}_{snd_id:08x}"));
                let mut _buf16 = [0u8; 0x10]; cur.read_exact(&mut _buf16)?;
                let idx_str = read_str(cur)?;
                let mut stap_item = BTreeMap::new();
                stap_item.insert("duration".into(), Value::Number(serde_yaml::Number::from(duration)));
                stap_item.insert("pitch1".into(), Value::Number(serde_yaml::Number::from(pitch1)));
                stap_item.insert("pitch2".into(), Value::Number(serde_yaml::Number::from(pitch2)));
                stap_item.insert("unknown2".into(), Value::Number(serde_yaml::Number::from(unknown2)));
                stap_item.insert("dynamics".into(), Value::Number(serde_yaml::Number::from(dynamics)));
                stap_item.insert("tempo".into(), Value::Number(serde_yaml::Number::from(tempo)));
                stap_item.insert("epr".into(), Value::Sequence(epr));
                stap_item.insert("fs".into(), Value::Number(serde_yaml::Number::from(fs)));
                stap_item.insert("snd".into(), snd);
                stap_item.insert("snd_length".into(), Value::Number(serde_yaml::Number::from(snd_length)));
                stap.insert(idx_str, Value::Mapping(stap_item.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
            }
            let mut artu = BTreeMap::new();
            artu.insert("phoneme".into(), Value::String(read_str(cur)?));
            artu.insert("stap".into(), Value::Mapping(stap.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
            sta.insert(idx, artu);
        }
        let _ = read_str(cur)?;
        let _ = read_str(cur)?;
        Ok(sta)
    }
    fn read_art<R: Read + Seek>(&self, cur: &mut R) -> Result<BTreeMap<u32, ArtType>> {
        let mut art = BTreeMap::new();
        let _ = read_u64_le(cur)?;
        let _ = read_arr(cur)?;
        loop {
            let mut head = [0u8; 8];
            if cur.read_exact(&mut head).is_err() { break; }
            if head != [0u8; 8] && head != [0xffu8; 8] {
                let pos = cur.stream_position()? - 8;
                cur.seek(SeekFrom::Start(pos))?;
                if read_str(cur).unwrap_or_default() == "articulation" { break; }
                continue;
            }
            let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
            if &sig != b"ART " { continue; }
            let (idx, block) = self.read_art_block(cur)?;
            art.insert(idx, block);
        }
        Ok(art)
    }
    fn read_art_block<R: Read + Seek>(&self, cur: &mut R) -> Result<(u32, ArtType)> {
        let mut block = BTreeMap::new();
        let _ = read_u32_le(cur)?;
        let _ = read_u32_le(cur)?;
        let _ = read_u32_le(cur)?;
        let idx = read_u32_le(cur)?;
        let n_artu = read_u32_le(cur)?;
        let mut artu = BTreeMap::new();
        for _ in 0..n_artu {
            let _ = read_u64_le(cur)?;
            let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
            if &sig == b"ART " {
                let (sub_idx, sub_block) = self.read_art_block(cur)?;
                let mut sub_art_map = BTreeMap::new();
                sub_art_map.insert(sub_idx.to_string(), Value::Mapping(sub_block.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
                block.insert("art".into(), Value::Mapping(sub_art_map.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
                continue;
            }
            let _ = read_u32_le(cur)?;
            let _ = read_u32_le(cur)?;
            let _ = read_u32_le(cur)?;
            let au_idx = read_u32_le(cur)?;
            let _ = read_u64_le(cur)?;
            let _ = read_u32_le(cur)?;
            let _ = read_u32_le(cur)?;
            let n_artp = read_u32_le(cur)?;
            let mut artp_map = BTreeMap::new();
            for _ in 0..n_artp {
                let dev_off = read_u64_le(cur)?;
                let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
                let _ = read_u32_le(cur)?;
                let _ = read_u32_le(cur)?;
                let _ = read_u32_le(cur)?;
                let duration = read_f64_le(cur)?;
                let _ = read_u16_le(cur)?;
                let pitch1 = read_f32_le(cur)?;
                let pitch2 = read_f32_le(cur)?;
                let unknown2 = read_f32_le(cur)?;
                let dynamics = read_f32_le(cur)?;
                let tempo = read_f32_le(cur)?;
                let _ = read_u32_le(cur)?;
                let artp_idx = read_u64_le(cur)?;
                let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
                let _ = read_u32_le(cur)?;
                let _ = read_str(cur)?;
                let _ = read_u32_le(cur)?;
                let _ = read_u32_le(cur)?;
                let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
                let _ = read_u32_le(cur)?;
                let _ = read_str(cur)?;
                let loc = cur.stream_position()?;
                let epr_num = match read_u32_le(cur) {
                    Ok(n) => n,
                    Err(_) => {
                        cur.seek(SeekFrom::Start(loc))?;
                        let mut _buf4 = [0u8; 4]; cur.read_exact(&mut _buf4)?;
                        read_u32_le(cur)?
                    }
                };
                let mut epr = Vec::new();
                for _ in 0..epr_num {
                    let pos = cur.stream_position()?;
                    let off = read_u64_le(cur)?;
                    epr.push(Value::String(format!("{pos:08x}={off:08x}")));
                }
                let fs = read_u32_le(cur)?;
                let _ = read_u16_le(cur)?;
                let snd_id = read_u32_le(cur)?;
                let snd_pos = cur.stream_position()?;
                let snd_off = read_u64_le(cur)?;
                let snd = Value::String(format!("{snd_pos:08x}={:016x}_{snd_id:08x}", snd_off - 0x12));
                let snd2_pos = cur.stream_position()?;
                let snd2_off = read_u64_le(cur)?;
                let snd_start = Value::String(format!("{snd2_pos:08x}={:016x}_{snd_id:08x}", snd2_off - 0x12));
                let cur_pos = cur.stream_position()? as usize;
                let slice_end = (cur_pos + 1024).min(self.ddi_bytes.len());
                let ddi_slice = &self.ddi_bytes[cur_pos..slice_end];
                let frame_align = if let Some(align_pos) = memmem::find(ddi_slice, b"default") {
                    let align_length = align_pos - 4;
                    let mut align_bytes = vec![0u8; align_length];
                    cur.read_exact(&mut align_bytes)?;
                    if align_length > 4 {
                        let group_num = u32::from_le_bytes(align_bytes[0..4].try_into()?);
                        let mut align_cur = Cursor::new(&align_bytes[4..]);
                        let mut groups = Vec::new();
                        for _ in 0..group_num {
                            let mut group = BTreeMap::new();
                            group.insert("start".into(), Value::Number(serde_yaml::Number::from(read_u32_le(&mut align_cur)?)));
                            group.insert("end".into(), Value::Number(serde_yaml::Number::from(read_u32_le(&mut align_cur)?)));
                            group.insert("start2".into(), Value::Number(serde_yaml::Number::from(read_u32_le(&mut align_cur)?)));
                            group.insert("end2".into(), Value::Number(serde_yaml::Number::from(read_u32_le(&mut align_cur)?)));
                            groups.push(Value::Mapping(group.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
                        }
                        groups
                    } else {
                        let vals = align_bytes.chunks(4)
                            .map(|c| Value::Number(serde_yaml::Number::from(u32::from_le_bytes(c.try_into().unwrap_or_default()))))
                            .collect::<Vec<_>>();
                        vec![Value::Sequence(vals)]
                    }
                } else {
                    vec![]
                };
                let _ = read_str(cur)?;
                let mut artp_item = BTreeMap::new();
                artp_item.insert("dev_artp".into(), Value::String(format!("{dev_off:08x}")));
                artp_item.insert("duration".into(), Value::Number(serde_yaml::Number::from(duration)));
                artp_item.insert("pitch1".into(), Value::Number(serde_yaml::Number::from(pitch1)));
                artp_item.insert("pitch2".into(), Value::Number(serde_yaml::Number::from(pitch2)));
                artp_item.insert("unknown2".into(), Value::Number(serde_yaml::Number::from(unknown2)));
                artp_item.insert("dynamics".into(), Value::Number(serde_yaml::Number::from(dynamics)));
                artp_item.insert("tempo".into(), Value::Number(serde_yaml::Number::from(tempo)));
                artp_item.insert("epr".into(), Value::Sequence(epr));
                artp_item.insert("fs".into(), Value::Number(serde_yaml::Number::from(fs)));
                artp_item.insert("snd".into(), snd);
                artp_item.insert("snd_start".into(), snd_start);
                artp_item.insert("frame_align".into(), Value::Sequence(frame_align));
                artp_map.insert(artp_idx.to_string(), Value::Mapping(artp_item.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
            }
            let mut au_map = BTreeMap::new();
            au_map.insert("phoneme".into(), Value::String(read_str(cur)?));
            au_map.insert("artp".into(), Value::Mapping(artp_map.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
            artu.insert(au_idx.to_string(), Value::Mapping(au_map.into_iter().map(|(k, v)| (Value::String(k), v)).collect()));
        }
        block.insert("phoneme".into(), Value::String(read_str(cur)?));
        if !artu.is_empty() { block.insert("artu".into(), Value::Mapping(artu.into_iter().map(|(k, v)| (Value::String(k), v)).collect())); }
        Ok((idx, block))
    }
    fn read_vqm<R: Read + Seek>(cur: &mut R) -> Result<BTreeMap<u32, ArtpType>> {
        let mut vqm = BTreeMap::new();
        let mut _buf8 = [0u8; 8]; cur.read_exact(&mut _buf8)?;
        let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
        let _ = read_u32_le(cur)?;
        let _ = read_u32_le(cur)?;
        let _ = read_u32_le(cur)?;
        let _ = read_u32_le(cur)?;
        let mut _buf8 = [0u8; 8]; cur.read_exact(&mut _buf8)?;
        let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
        let _ = read_u32_le(cur)?;
        let _ = read_u32_le(cur)?;
        let _ = read_u32_le(cur)?;
        let num = read_u32_le(cur)?;
        let _ = read_u32_le(cur)?;
        for _ in 0..num {
            let mut _buf8 = [0u8; 8]; cur.read_exact(&mut _buf8)?;
            let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
            let _ = read_u32_le(cur)?;
            let _ = read_u32_le(cur)?;
            let _ = read_u32_le(cur)?;
            let duration = read_f64_le(cur)?;
            let _ = read_u16_le(cur)?;
            let pitch1 = read_f32_le(cur)?;
            let pitch2 = read_f32_le(cur)?;
            let unknown2 = read_f32_le(cur)?;
            let dynamics = read_f32_le(cur)?;
            let tempo = read_f32_le(cur)?;
            let _ = read_u32_le(cur)?;
            let mut _buf4 = [0u8; 4]; cur.read_exact(&mut _buf4)?;
            let epr_num = read_u32_le(cur)?;
            let mut epr = Vec::new();
            for _ in 0..epr_num {
                let pos = cur.stream_position()?;
                let off = read_u64_le(cur)?;
                epr.push(Value::String(format!("{pos:08x}={off:08x}")));
            }
            let fs = read_u32_le(cur)?;
            let _ = read_u16_le(cur)?;
            let snd_id = read_u32_le(cur)?;
            let snd_pos = cur.stream_position()?;
            let snd_off = read_u64_le(cur)?;
            let snd = Value::String(format!("{snd_pos:08x}={snd_off:016x}_{snd_id:08x}"));
            let mut _buf16 = [0u8; 0x10]; cur.read_exact(&mut _buf16)?;
            let idx_str = read_str(cur)?;
            let idx: u32 = idx_str.parse()?;
            let mut artp = BTreeMap::new();
            artp.insert("duration".into(), Value::Number(serde_yaml::Number::from(duration)));
            artp.insert("pitch1".into(), Value::Number(serde_yaml::Number::from(pitch1)));
            artp.insert("pitch2".into(), Value::Number(serde_yaml::Number::from(pitch2)));
            artp.insert("unknown2".into(), Value::Number(serde_yaml::Number::from(unknown2)));
            artp.insert("dynamics".into(), Value::Number(serde_yaml::Number::from(dynamics)));
            artp.insert("tempo".into(), Value::Number(serde_yaml::Number::from(tempo)));
            artp.insert("epr".into(), Value::Sequence(epr));
            artp.insert("fs".into(), Value::Number(serde_yaml::Number::from(fs)));
            artp.insert("snd".into(), snd);
            vqm.insert(idx, artp);
        }
        let _ = read_str(cur)?;
        let _ = read_str(cur)?;
        Ok(vqm)
    }
}