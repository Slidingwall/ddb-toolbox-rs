use crate::ddi::{str_to_data, stream_reverse_search, DDIModel};
use anyhow::{anyhow, Context, Result};
use memchr::memmem;
use std::fs::File;
use std::io::{copy, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
const DDI_FOOTER: &[u8] = b"\x05\x00\x00\x00voice";
#[derive(Debug, Default)]
pub struct VqmMeta {
    pub idx: String,
    pub epr: Vec<u64>,
    pub snd_id: u32,
    pub snd: u64,
    pub fs: u32,
    pub duration: f64,
    pub pitch2: f32,
    pub unknown2: f32,
    pub tempo: f32,
    pub dynamics: f32,
}
fn parse_hex_usize(value: &str) -> Result<usize> {
    usize::from_str_radix(value, 16).with_context(|| format!("Invalid hex number: {}", value))
}
fn parse_hex_u32(value: &str) -> Result<u32> {
    u32::from_str_radix(value, 16).with_context(|| format!("Invalid hex number: {}", value))
}
fn byte_replace(src: &[u8], off: usize, del_len: usize, rep: &[u8]) -> Vec<u8> {
    [&src[..off], rep, &src[off + del_len..]].concat()
}
fn create_vqm_stream(list: &[VqmMeta]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&[0xFF; 8]);
    buf.extend_from_slice(b"VQM ");
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&[0xFF; 8]);
    buf.extend_from_slice(b"VQMu");
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&(list.len() as u32).to_le_bytes());
    buf.extend_from_slice(&(list.len() as u32).to_le_bytes());
    for m in list {
        buf.extend_from_slice(&[0xFF; 8]);
        buf.extend_from_slice(b"VQMp");
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&m.duration.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&224.0f32.to_le_bytes());
        buf.extend_from_slice(&m.pitch2.to_le_bytes());
        buf.extend_from_slice(&m.unknown2.to_le_bytes());
        buf.extend_from_slice(&m.dynamics.to_le_bytes());
        buf.extend_from_slice(&m.tempo.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&[0xFF; 4]);
        buf.extend_from_slice(&(m.epr.len() as u32).to_le_bytes());
        buf.extend(m.epr.iter().flat_map(|&e| e.to_le_bytes()));
        buf.extend_from_slice(&m.fs.to_le_bytes());
        buf.extend_from_slice(&[1u8, 0u8]);
        buf.extend_from_slice(&m.snd_id.to_le_bytes());
        buf.extend_from_slice(&m.snd.to_le_bytes());
        buf.extend_from_slice(&[0xFF; 16]);
        buf.extend_from_slice(&str_to_data(&m.idx));
    }
    buf.extend_from_slice(&str_to_data("GROWL"));
    buf.extend_from_slice(&str_to_data("vqm"));
    buf
}
fn copy_segment(
    mixins_ddb: &mut File,
    output: &mut File,
    offset: usize,
    expected_sig: &[u8],
) -> Result<u64> {
    let out_off = output.stream_position()?;
    mixins_ddb.seek(SeekFrom::Start(offset as u64))?;
    let mut sig = [0u8; 4];
    mixins_ddb.read_exact(&mut sig)?;
    anyhow::ensure!(&sig == expected_sig, "Mixins DDB file is broken");
    let mut len_buf = [0u8; 4];
    mixins_ddb.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    mixins_ddb.seek(SeekFrom::Start(offset as u64))?;
    let mut bytes = vec![0u8; len];
    mixins_ddb.read_exact(&mut bytes)?;
    output.write_all(&bytes)?;
    Ok(out_off)
}
fn copy_epr(mixins_ddb: &mut File, output: &mut File, epr_str: &str) -> Result<u64> {
    let (_, off_str) = epr_str.split_once('=').unwrap();
    let off = parse_hex_usize(off_str)?;
    copy_segment(mixins_ddb, output, off, b"FRM2")
}
fn copy_snd_common(
    mixins_ddb: &mut File,
    output: &mut File,
    snd_str: &str,
    real_off: Option<usize>,
) -> Result<(u32, u64)> {
    let (_, t) = snd_str.split_once('=').unwrap();
    let (off_str, id_str) = t.split_once('_').unwrap();
    let off = parse_hex_usize(off_str)?;
    let id = parse_hex_u32(id_str)?;
    let actual_off = real_off.unwrap_or(off);
    let out_off = copy_segment(mixins_ddb, output, actual_off, b"SND ")?;
    Ok((id, out_off))
}
fn mixins_vqm(
    src_ddi: &[u8],
    output: &mut File,
    mixins_model: &DDIModel,
    mixins_ddb: &mut File,
) -> Result<Vec<u8>> {
    if !mixins_model.ddi_data_dict.contains_key("vqm") {
        return Err(anyhow!("Mixins DDI doesn't have vqm stream."));
    }
    let mut src_model = DDIModel::new(src_ddi.to_vec());
    src_model.read(None, false)?;
    let has_src_vqm = src_model.ddi_data_dict.contains_key("vqm");
    let mut meta_list = Vec::new();
    let mixins_vqm = mixins_model.vqm_data.as_ref().unwrap();
    for (idx, info) in mixins_vqm {
        let epr_list = info.get("epr")
            .and_then(|v| v.as_sequence())
            .unwrap()
            .iter()
            .map(|e| copy_epr(mixins_ddb, output, e.as_str().unwrap()))
            .collect::<Result<Vec<_>>>()?;
        let (snd_id, snd_off) = copy_snd_common(
            mixins_ddb,
            output,
            info.get("snd").and_then(|v| v.as_str()).unwrap(),
            None,
        )?;
        meta_list.push(VqmMeta {
            idx: idx.to_string(),
            epr: epr_list,
            snd_id,
            snd: snd_off,
            fs: info.get("fs").and_then(|v| v.as_u64()).unwrap() as u32,
            duration: info.get("duration").and_then(|v| v.as_f64()).unwrap(),
            pitch2: info.get("pitch2").and_then(|v| v.as_f64()).unwrap() as f32,
            unknown2: info.get("unknown2").and_then(|v| v.as_f64()).unwrap() as f32,
            tempo: info.get("tempo").and_then(|v| v.as_f64()).unwrap() as f32,
            dynamics: info.get("dynamics").and_then(|v| v.as_f64()).unwrap() as f32,
        });
    }
    let vqm_stream = create_vqm_stream(&meta_list);
    let (vqm_pos, vqm_end) = if has_src_vqm {
        src_model.offset_map["vqm"]
    } else {
        let pos = memmem::find(src_ddi, DDI_FOOTER).unwrap();
        (pos, pos)
    };
    let mut src_ddi_mut = src_ddi.to_vec();
    if !has_src_vqm {
        let dbv_off = src_model.offset_map["dbv"].0;
        let dbv_len_post = dbv_off + 0x18;
        let mut cur = Cursor::new(&mut src_ddi_mut);
        cur.seek(SeekFrom::Start(dbv_len_post as u64))?;
        let mut len_buf = [0u8; 4];
        cur.read_exact(&mut len_buf)?;
        let dbv_len = u32::from_le_bytes(len_buf) + 1;
        cur.seek(SeekFrom::Start(dbv_len_post as u64))?;
        cur.write_all(&dbv_len.to_le_bytes())?;
    }
    Ok(byte_replace(
        &src_ddi_mut,
        vqm_pos,
        vqm_end - vqm_pos,
        &vqm_stream,
    ))
}
fn mixins_sta2vqm(
    src_ddi: &[u8],
    output: &mut File,
    mixins_model: &DDIModel,
    mixins_ddb: &mut File,
    sta2vqm_phoneme: &str,
) -> Result<Vec<u8>> {
    let mut src_model = DDIModel::new(src_ddi.to_vec());
    src_model.read(None, false)?;
    let has_src_vqm = src_model.ddi_data_dict.contains_key("vqm");
    let mixins_sta = mixins_model
        .sta_data
        .values()
        .find(|s| s.get("phoneme").and_then(|v| v.as_str()).unwrap_or_default() == sta2vqm_phoneme)
        .ok_or_else(|| {
            anyhow!(
                "Mixins DDI doesn't have stationary entry for phoneme \"{}\"",
                sta2vqm_phoneme
            )
        })?;
    let mut meta_list = Vec::new();
    let stap_map = mixins_sta.get("stap").and_then(|v| v.as_mapping()).unwrap();
    for (idx, sta_item) in stap_map.values().enumerate() {
        let epr_arr = sta_item.get("epr").and_then(|v| v.as_sequence()).unwrap();
        if epr_arr.len() < 100 {
            println!(
                "Warning: EpR count is less than 100, EpR count: {}",
                epr_arr.len()
            );
            continue;
        }
        let epr_list = epr_arr[0..100]
            .iter()
            .map(|e| copy_epr(mixins_ddb, output, e.as_str().unwrap()))
            .collect::<Result<Vec<_>>>()?;
        let snd_str = sta_item.get("snd").and_then(|v| v.as_str()).unwrap();
        let (_, t) = snd_str.split_once('=').unwrap();
        let (off_str, _) = t.split_once('_').unwrap();
        let off = parse_hex_usize(off_str)?;
        let real_off = stream_reverse_search(mixins_ddb, b"SND ", off, -1);
        println!("Delta SND offset: {:08x}", off - real_off);
        let (snd_id, snd_off) = copy_snd_common(mixins_ddb, output, snd_str, Some(real_off))?;
        meta_list.push(VqmMeta {
            idx: idx.to_string(),
            epr: epr_list,
            snd_id,
            snd: snd_off,
            fs: sta_item.get("fs").and_then(|v| v.as_u64()).unwrap() as u32,
            duration: sta_item.get("duration").and_then(|v| v.as_f64()).unwrap(),
            pitch2: sta_item.get("pitch2").and_then(|v| v.as_f64()).unwrap() as f32,
            unknown2: sta_item.get("unknown2").and_then(|v| v.as_f64()).unwrap() as f32,
            tempo: sta_item.get("tempo").and_then(|v| v.as_f64()).unwrap() as f32,
            dynamics: sta_item.get("dynamics").and_then(|v| v.as_f64()).unwrap() as f32,
        });
    }
    let vqm_stream = create_vqm_stream(&meta_list);
    let (vqm_pos, vqm_end) = if has_src_vqm {
        src_model.offset_map["vqm"]
    } else {
        let pos = memmem::find(src_ddi, DDI_FOOTER).unwrap();
        (pos, pos)
    };
    let mut src_ddi_mut = src_ddi.to_vec();
    if !has_src_vqm {
        let dbv_off = src_model.offset_map["dbv"].0;
        let dbv_len_post = dbv_off + 0x18;
        let mut cur = Cursor::new(&mut src_ddi_mut);
        cur.seek(SeekFrom::Start(dbv_len_post as u64))?;
        let mut len_buf = [0u8; 4];
        cur.read_exact(&mut len_buf)?;
        let dbv_len = u32::from_le_bytes(len_buf) + 1;
        cur.seek(SeekFrom::Start(dbv_len_post as u64))?;
        cur.write_all(&dbv_len.to_le_bytes())?;
    }
    Ok(byte_replace(
        &src_ddi_mut,
        vqm_pos,
        vqm_end - vqm_pos,
        &vqm_stream,
    ))
}
pub fn main(
    src_path: &Path,
    mixins_path: Option<&Path>,
    dst_path: Option<&Path>,
    mixins_item: &str,
    sta2vqm_phoneme: &str,
) -> Result<()> {
    let src_dir = src_path.parent().context("invalid path")?;
    let src_name = src_path
        .file_stem()
        .context("invalid file")?
        .to_str()
        .context("invalid name")?;
    let mixins_path = mixins_path.unwrap_or(src_path);
    let mixins_dir = mixins_path.parent().context("invalid path")?;
    let mixins_name = mixins_path
        .file_stem()
        .context("invalid mixins file")?
        .to_str()
        .context("invalid mixins name")?;
    let dst_path = dst_path.map_or_else(|| src_dir.join("mixins"), PathBuf::from);
    std::fs::create_dir_all(&dst_path)?;
    let src_ddb = src_dir.join(format!("{src_name}.ddb"));
    let src_ddi = src_dir.join(format!("{src_name}.ddi"));
    let mixins_ddb = mixins_dir.join(format!("{mixins_name}.ddb"));
    let mixins_ddi = mixins_dir.join(format!("{mixins_name}.ddi"));
    anyhow::ensure!(src_ddb.exists(), "Source ddb file not exists.");
    anyhow::ensure!(src_ddi.exists(), "Source ddi file not exists.");
    anyhow::ensure!(mixins_ddb.exists(), "Mixins ddb file not exists.");
    anyhow::ensure!(mixins_ddi.exists(), "Mixins ddi file not exists.");
    let src_ddi_bytes = std::fs::read(src_ddi)?;
    let mixins_ddi_bytes = std::fs::read(mixins_ddi)?;
    println!("Reading mixins DDI...");
    let mut mixins_model = DDIModel::new(mixins_ddi_bytes);
    mixins_model.read(None, false)?;
    println!("Creating DDB...");
    let dst_ddb_path = dst_path.join(format!("{src_name}.ddb"));
    let mut dst_ddb = File::create(&dst_ddb_path)?;
    copy(&mut File::open(&src_ddb)?, &mut dst_ddb)?;
    let mut mixins_ddb_stream = File::open(&mixins_ddb)?;
    let final_ddi = match mixins_item {
        "vqm" => mixins_vqm(
            &src_ddi_bytes,
            &mut dst_ddb,
            &mixins_model,
            &mut mixins_ddb_stream,
        )?,
        "sta2vqm" => mixins_sta2vqm(
            &src_ddi_bytes,
            &mut dst_ddb,
            &mixins_model,
            &mut mixins_ddb_stream,
            sta2vqm_phoneme,
        )?,
        _ => return Err(anyhow!("Invalid mixins_item: {}, must be 'vqm' or 'sta2vqm'", mixins_item)),
    };
    println!("Creating DDI...");
    std::fs::write(dst_path.join(format!("{src_name}.ddi")), final_ddi)?;
    println!("Finished...");
    Ok(())
}