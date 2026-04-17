use anyhow::Result;
use memchr::memmem;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

pub type ArtpType = Map<String, Value>;
pub type ArtuType = Map<String, Value>;
pub type ArtType = Map<String, Value>;

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

// 纯Python原版：无任何新增校验/功能
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

// 纯Python原版逻辑
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

        // PHDC
        if let Some(pos) = memmem::find(&self.ddi_bytes, b"PHDC") {
            cur.seek(SeekFrom::Start(pos as u64))?;
            self.phdc_data = Self::read_phdc(&mut cur)?;
            self.offset_map.insert("phdc".into(), (pos, cur.stream_position()? as usize));
        }

        // TDB
        let tdb_sig = [0xffu8; 8].into_iter().chain(b"TDB ".iter().copied()).collect::<Vec<_>>();
        if let Some(pos) = memmem::find(&self.ddi_bytes, &tdb_sig) {
            cur.seek(SeekFrom::Start(pos as u64))?;
            self.tdb_data = Self::read_tdb(&mut cur)?;
            self.offset_map.insert("tdb".into(), (pos, cur.stream_position()? as usize));
        }

        // DBV
        let dbv_sig = [0x00u8; 8].into_iter().chain(b"DBV ".iter().copied()).collect::<Vec<_>>();
        if let Some(pos) = memmem::find(&self.ddi_bytes, &dbv_sig) {
            cur.seek(SeekFrom::Start(pos as u64))?;
            Self::read_dbv(&mut cur)?;
            self.offset_map.insert("dbv".into(), (pos, cur.stream_position()? as usize));
        }

        // STA
        let sta_sig = [0x00u8; 8].into_iter().chain(b"STA ".iter().copied()).collect::<Vec<_>>();
        if let Some(pos) = memmem::find(&self.ddi_bytes, &sta_sig) {
            let arr_pos = reverse_search(&self.ddi_bytes, b"ARR ", pos, -1);
            let sta_offset = arr_pos - 8;
            cur.seek(SeekFrom::Start(sta_offset as u64))?;
            self.sta_data = Self::read_sta(&mut cur)?;
            self.offset_map.insert("sta".into(), (sta_offset, cur.stream_position()? as usize));
        }

        // ART
        let art_sig = [0x00u8; 8].into_iter().chain(b"ART ".iter().copied()).collect::<Vec<_>>();
        if let Some(pos) = memmem::find(&self.ddi_bytes, &art_sig) {
            let arr_pos = reverse_search(&self.ddi_bytes, b"ARR ", pos, -1);
            let art_offset = arr_pos - 8;
            cur.seek(SeekFrom::Start(art_offset as u64))?;
            self.art_data = self.read_art(&mut cur)?;
            self.offset_map.insert("art".into(), (art_offset, cur.stream_position()? as usize));
        }

        // VQM
        let vqm_sig = [0xffu8; 8].into_iter().chain(b"VQM ".iter().copied()).collect::<Vec<_>>();
        if let Some(pos) = memmem::find(&self.ddi_bytes, &vqm_sig) {
            cur.seek(SeekFrom::Start(pos as u64))?;
            self.vqm_data = Some(Self::read_vqm(&mut cur)?);
            self.offset_map.insert("vqm".into(), (pos, cur.stream_position()? as usize));
        }

        self.build_ddi_dict();
        Ok(())
    }

    // 纯Python原版字典构建
    fn build_ddi_dict(&mut self) {
        let mut ddi_dict = BTreeMap::new();

        // STA
        let mut sta_dict = BTreeMap::new();
        for stau in self.sta_data.values() {
            let phoneme = stau["phoneme"].as_str().unwrap_or_default();
            let mut items = Vec::new();
            for stap in stau["stap"].as_object().unwrap().values() {
                items.push(json!({
                    "snd": stap["snd"], "epr": stap["epr"], "pitch": stap["pitch1"]
                }));
            }
            sta_dict.insert(phoneme.to_string(), items);
        }
        ddi_dict.insert("sta".to_string(), json!(sta_dict));

        // ART
        let mut art_dict = BTreeMap::new();
        for art in self.art_data.values() {
            if let Some(artu) = art.get("artu").and_then(|v| v.as_object()) {
                for au in artu.values() {
                    let key = format!("{} {}", art["phoneme"].as_str().unwrap_or_default(), au["phoneme"].as_str().unwrap_or_default());
                    let mut items = Vec::new();
                    for artp in au["artp"].as_object().unwrap().values() {
                        items.push(json!({
                            "snd": artp["snd"], "snd_start": artp["snd_start"], "epr": artp["epr"], "pitch": artp["pitch1"]
                        }));
                    }
                    art_dict.insert(key, items);
                }
            }
            if let Some(sub_art) = art.get("art").and_then(|v| v.as_object()) {
                for sub in sub_art.values() {
                    if let Some(artu) = sub.get("artu").and_then(|v| v.as_object()) {
                        for au in artu.values() {
                            let key = format!("{} {} {}", art["phoneme"].as_str().unwrap_or_default(), sub["phoneme"].as_str().unwrap_or_default(), au["phoneme"].as_str().unwrap_or_default());
                            let mut items = Vec::new();
                            for artp in au["artp"].as_object().unwrap().values() {
                                items.push(json!({
                                    "snd": artp["snd"], "snd_start": artp["snd_start"], "epr": artp["epr"], "pitch": artp["pitch1"]
                                }));
                            }
                            art_dict.insert(key, items);
                        }
                    }
                }
            }
        }
        ddi_dict.insert("art".to_string(), json!(art_dict));

        // VQM
        if let Some(vqm) = &self.vqm_data {
            let mut vqm_items = Vec::new();
            for v in vqm.values() {
                vqm_items.push(json!({
                    "snd": v["snd"], "epr": v["epr"], "pitch": v["pitch1"]
                }));
            }
            ddi_dict.insert("vqm".to_string(), json!(vqm_items));
        }

        self.ddi_data_dict = ddi_dict;
    }

    // 纯Python原版，无任何新增校验
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
            if buf[0x1E] == 0 { voiced.push(s) } else { unvoiced.push(s) }
        }
        phdc.insert("phoneme".into(), json!({ "voiced": voiced, "unvoiced": unvoiced }));

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
                inner.insert(idx.to_string(), json!(val));
            }
            let _ = read_u32_le(cur)?;
            phg2_data.insert(key, json!(inner));
        }
        phdc.insert("phg2".into(), json!(phg2_data));

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
                epr_list.push(json!(val));
                offset += 8;
            }
            epr_guide_data.insert(key, json!(epr_list));
        }
        phdc.insert("epr_guide".into(), json!(epr_guide_data));
        Ok(phdc)
    }

    fn read_tdb<R: Read + Seek>(cur: &mut R) -> Result<BTreeMap<u32, String>> {
        let mut tdb = BTreeMap::new();
        let _ = cur.read_exact(&mut [0xffu8; 8]);
        let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
        let _ = read_u32_le(cur)?;
        let _ = read_u64_le(cur)?;
        let num = read_u32_le(cur)?;

        for _ in 0..num {
            let _ = cur.read_exact(&mut [0xffu8; 8]);
            let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
            let _ = read_u32_le(cur)?;
            let _ = read_u64_le(cur)?;
            let idx = read_u32_le(cur)?;
            let sn = read_u32_le(cur)?;

            for _ in 0..sn {
                let _ = cur.read_exact(&mut [0xffu8; 8]);
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
            let _ = cur.read_exact(&mut [0xffu8; 8]);
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
                let _ = cur.read_exact(&mut [0u8; 4]);
                let epr_num = read_u32_le(cur)?;

                let mut epr = Vec::new();
                for _ in 0..epr_num {
                    let pos = cur.stream_position()?;
                    let off = read_u64_le(cur)?;
                    epr.push(format!("{pos:08x}={off:08x}"));
                }

                let fs = read_u32_le(cur)?;
                let _ = read_u16_le(cur)?;
                let snd_id = read_u32_le(cur)?;
                let snd_pos = cur.stream_position()?;
                let snd_off = read_u64_le(cur)?;
                let snd = format!("{snd_pos:08x}={snd_off:016x}_{snd_id:08x}");
                let _ = cur.read_exact(&mut [0u8; 0x10]);
                let idx_str = read_str(cur)?;

                stap.insert(idx_str, json!({
                    "duration": duration, "pitch1": pitch1, "pitch2": pitch2,
                    "unknown2": unknown2, "dynamics": dynamics, "tempo": tempo,
                    "epr": epr, "fs": fs, "snd": snd, "snd_length": snd_length
                }));
            }

            let mut artu = Map::new();
            artu.insert("phoneme".into(), json!(read_str(cur)?));
            artu.insert("stap".into(), json!(stap));
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
        let mut block = Map::new();
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
                block.insert("art".into(), json!({sub_idx.to_string(): sub_block}));
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
                        cur.read_exact(&mut [0u8; 4])?;
                        read_u32_le(cur)?
                    }
                };

                let mut epr = Vec::new();
                for _ in 0..epr_num {
                    let pos = cur.stream_position()?;
                    let off = read_u64_le(cur)?;
                    epr.push(format!("{pos:08x}={off:08x}"));
                }

                let fs = read_u32_le(cur)?;
                let _ = read_u16_le(cur)?;
                let snd_id = read_u32_le(cur)?;
                let snd_pos = cur.stream_position()?;
                let snd_off = read_u64_le(cur)?;
                let snd = format!("{snd_pos:08x}={:016x}_{snd_id:08x}", snd_off - 0x12);
                let snd2_pos = cur.stream_position()?;
                let snd2_off = read_u64_le(cur)?;
                let snd_start = format!("{snd2_pos:08x}={:016x}_{snd_id:08x}", snd2_off - 0x12);

                let cur_pos = cur.stream_position()? as usize;
                let slice = &self.ddi_bytes[cur_pos..cur_pos + 1024];
                let align_pos = memmem::find(slice, b"default").unwrap();
                let align_length = align_pos - 4;
                let mut align_bytes = vec![0u8; align_length];
                cur.read_exact(&mut align_bytes)?;

                let frame_align = if align_length > 4 {
                    let group_num = u32::from_le_bytes(align_bytes[0..4].try_into()?);
                    let mut align_cur = Cursor::new(&align_bytes[4..]);
                    let mut groups = Vec::new();
                    for _ in 0..group_num {
                        groups.push(json!({
                            "start": read_u32_le(&mut align_cur)?,
                            "end": read_u32_le(&mut align_cur)?,
                            "start2": read_u32_le(&mut align_cur)?,
                            "end2": read_u32_le(&mut align_cur)?,
                        }));
                    }
                    groups
                } else {
                    let vals = align_bytes.chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap_or_default())).collect::<Vec<_>>();
                    vec![json!(vals)]
                };

                let _ = read_str(cur)?;
                artp_map.insert(artp_idx.to_string(), json!({
                    "dev_artp": format!("{dev_off:08x}"), "duration": duration, "pitch1": pitch1,
                    "pitch2": pitch2, "unknown2": unknown2, "dynamics": dynamics,
                    "tempo": tempo, "epr": epr, "fs": fs, "snd": snd,
                    "snd_start": snd_start, "frame_align": frame_align
                }));
            }

            artu.insert(au_idx.to_string(), json!({
                "phoneme": read_str(cur)?,
                "artp": artp_map
            }));
        }

        block.insert("phoneme".into(), json!(read_str(cur)?));
        if !artu.is_empty() { block.insert("artu".into(), json!(artu)); }
        Ok((idx, block))
    }

    fn read_vqm<R: Read + Seek>(cur: &mut R) -> Result<BTreeMap<u32, ArtpType>> {
        let mut vqm = BTreeMap::new();
        let _ = cur.read_exact(&mut [0xffu8; 8]);
        let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
        let _ = read_u32_le(cur)?;
        let _ = read_u32_le(cur)?;
        let _ = read_u32_le(cur)?;
        let _ = read_u32_le(cur)?;
        let _ = cur.read_exact(&mut [0xffu8; 8]);

        let mut sig = [0u8; 4]; cur.read_exact(&mut sig)?;
        let _ = read_u32_le(cur)?;
        let _ = read_u32_le(cur)?;
        let _ = read_u32_le(cur)?;
        let num = read_u32_le(cur)?;
        let _ = read_u32_le(cur)?;

        for _ in 0..num {
            let _ = cur.read_exact(&mut [0xffu8; 8]);
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
            let epr_num = read_u32_le(cur)?;
            let mut epr = Vec::new();
            for _ in 0..epr_num {
                let pos = cur.stream_position()?;
                let off = read_u64_le(cur)?;
                epr.push(format!("{pos:08x}={off:08x}"));
            }

            let fs = read_u32_le(cur)?;
            let _ = read_u16_le(cur)?;
            let snd_id = read_u32_le(cur)?;
            let snd_pos = cur.stream_position()?;
            let snd_off = read_u64_le(cur)?;
            let snd = format!("{snd_pos:08x}={snd_off:016x}_{snd_id:08x}");
            let _ = cur.read_exact(&mut [0u8; 0x10]);
            let idx: u32 = read_str(cur)?.parse()?;

            let mut artp = Map::new();
            artp.insert("duration".into(), json!(duration));
            artp.insert("pitch1".into(), json!(pitch1));
            artp.insert("pitch2".into(), json!(pitch2));
            artp.insert("unknown2".into(), json!(unknown2));
            artp.insert("dynamics".into(), json!(dynamics));
            artp.insert("tempo".into(), json!(tempo));
            artp.insert("epr".into(), json!(epr));
            artp.insert("fs".into(), json!(fs));
            artp.insert("snd".into(), json!(snd));
            vqm.insert(idx, artp);
        }
        let _ = read_str(cur)?;
        let _ = read_str(cur)?;
        Ok(vqm)
    }
}

// 仅保留Python必需的辅助函数
fn read_u16_le<R: Read>(cur: &mut R) -> Result<u16> {
    let mut buf = [0u8; 2];
    cur.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
}