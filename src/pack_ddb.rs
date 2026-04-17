use crate::ddi::{DDIModel, reverse_search};
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;
fn escape_filename(filename: &str) -> String {
    filename
        .chars()
        .map(|c| match c.is_ascii_lowercase() {
            true => c.to_string(),
            false => format!("%{}%", c as u32),
        })
        .collect()
}
fn parse_hex_usize(value: &str) -> Result<usize> {
    usize::from_str_radix(value, 16).with_context(|| format!("Invalid hex number: {}", value))
}
fn copy_chunk_bytes<R: Read + Seek>(
    reader: &mut R,
    output: &mut File,
    offset: usize,
    expected_sig: &[u8],
) -> Result<u64> {
    let out_pos = output.stream_position()?;
    reader.seek(SeekFrom::Start(offset as u64))?;
    let mut header = [0u8; 4];
    reader.read_exact(&mut header)?;
    anyhow::ensure!(&header == expected_sig, "Articulation file is broken");
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as u64;
    reader.seek(SeekFrom::Start(offset as u64))?;
    let mut chunk = reader.take(len);
    std::io::copy(&mut chunk, output)?;
    Ok(out_pos)
}
pub fn main(src_path: &Path, dst_path: Option<&Path>) -> Result<()> {
    anyhow::ensure!(src_path.exists(), "singer tree file not exists");
    let singer_path = src_path.with_extension("");
    let singer_name = singer_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid singer name"))?;
    let dst_path = match dst_path {
        Some(p) => p.to_path_buf(),
        None => src_path
            .parent()
            .unwrap()
            .join(src_path.file_stem().unwrap())
            .join(singer_name),
    };
    std::fs::create_dir_all(&dst_path)?;
    let ddi_bytes = std::fs::read(src_path)?;
    let mut ddi_data = Cursor::new(ddi_bytes);
    let ddi_path = dst_path.join(format!("{}.ddi", singer_name));
    let ddb_path = dst_path.join(format!("{}.ddb", singer_name));
    if ddi_path.exists() {
        std::fs::remove_file(&ddi_path)?;
    }
    if ddb_path.exists() {
        std::fs::remove_file(&ddb_path)?;
    }
    println!("Reading DDI...");
    let mut ddi_model = DDIModel::new(ddi_data.get_ref().to_vec());
    ddi_model.read(None, false)?;
    println!("Creating DDB...");
    let mut ddb_f = File::create(&ddb_path)?;
    let art_dict = ddi_model.ddi_data_dict["art"]
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Invalid ART data structure"))?;
    for (cvvc, art_items) in art_dict {
        let phonemes: Vec<_> = cvvc.split(' ').collect();
        let mut art_file = singer_path.join("voice").join("articulation");
        for phoneme in phonemes.iter() {
            art_file.push(escape_filename(phoneme));
        }
        println!("Adding art file: {}", art_file.display());
        anyhow::ensure!(
            art_file.exists(),
            "Articulation file {} not found",
            art_file.display()
        );
        let art_bytes = std::fs::read(&art_file)?;
        let mut art_data = Cursor::new(&art_bytes);
        for art_item in art_items.as_array().unwrap() {
            for epr_info in art_item["epr"].as_array().unwrap() {
                let epr_str = epr_info.as_str().unwrap();
                let (ddi_epr_pos_str, epr_offset_str) = epr_str.split_once('=').unwrap();
                let ddi_epr_pos = parse_hex_usize(ddi_epr_pos_str)?;
                let epr_offset = parse_hex_usize(epr_offset_str)?;
                let ddb_epr_offset =
                    copy_chunk_bytes(&mut art_data, &mut ddb_f, epr_offset, b"FRM2")?;
                ddi_data.seek(SeekFrom::Start(ddi_epr_pos as u64))?;
                ddi_data.write_all(&ddb_epr_offset.to_le_bytes())?;
            }
            let snd_str = art_item["snd"].as_str().unwrap();
            let (ddi_snd_pos_str, t) = snd_str.split_once('=').unwrap();
            let (snd_offset_str, _) = t.split_once('_').unwrap();
            let snd_start_str = art_item["snd_start"].as_str().unwrap();
            let (_ddi_snd_pos2_str, t2) = snd_start_str.split_once('=').unwrap();
            let (snd_offset2_str, _) = t2.split_once('_').unwrap();
            let ddi_snd_pos = parse_hex_usize(ddi_snd_pos_str)?;
            let snd_offset = parse_hex_usize(snd_offset_str)?;
            let snd_offset2 = parse_hex_usize(snd_offset2_str)?;
            let offset2_delta = snd_offset2 - snd_offset;
            let ddb_snd_offset = ddb_f.stream_position()? + 0x12;
            copy_chunk_bytes(&mut art_data, &mut ddb_f, snd_offset, b"SND ")?;
            ddi_data.seek(SeekFrom::Start(ddi_snd_pos as u64))?;
            ddi_data.write_all(&ddb_snd_offset.to_le_bytes())?;
            ddi_data.write_all(&(ddb_snd_offset + offset2_delta as u64).to_le_bytes())?;
        }
    }
    let sta_dict = ddi_model.ddi_data_dict["sta"]
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Invalid STA data structure"))?;
    for (phoneme, sta_items) in sta_dict {
        for (idx, sta_item) in sta_items.as_array().unwrap().iter().enumerate() {
            let sta_file = singer_path
                .join("voice/stationary/normal")
                .join(escape_filename(phoneme))
                .join(escape_filename(&idx.to_string()));
            println!("Adding sta file: {}", sta_file.display());
            anyhow::ensure!(
                sta_file.exists(),
                "Stationary file {} not found",
                sta_file.display()
            );
            let sta_bytes = std::fs::read(&sta_file)?;
            let mut sta_data = Cursor::new(&sta_bytes);
            for epr_info in sta_item["epr"].as_array().unwrap() {
                let epr_str = epr_info.as_str().unwrap();
                let (ddi_epr_pos_str, epr_offset_str) = epr_str.split_once('=').unwrap();
                let ddi_epr_pos = parse_hex_usize(ddi_epr_pos_str)?;
                let epr_offset = parse_hex_usize(epr_offset_str)?;
                let ddb_epr_offset =
                    copy_chunk_bytes(&mut sta_data, &mut ddb_f, epr_offset, b"FRM2")?;
                ddi_data.seek(SeekFrom::Start(ddi_epr_pos as u64))?;
                ddi_data.write_all(&ddb_epr_offset.to_le_bytes())?;
            }
            let snd_str = sta_item["snd"].as_str().unwrap();
            let (ddi_snd_pos_str, t) = snd_str.split_once('=').unwrap();
            let (snd_offset_str, _) = t.split_once('_').unwrap();
            let ddi_snd_pos = parse_hex_usize(ddi_snd_pos_str)?;
            let snd_offset = parse_hex_usize(snd_offset_str)?;
            let real_snd_offset = reverse_search(&sta_bytes, b"SND ", snd_offset, -1);
            let delta_snd_offset = snd_offset - real_snd_offset;
            let ddb_snd_offset = ddb_f.stream_position()? + delta_snd_offset as u64;
            copy_chunk_bytes(&mut sta_data, &mut ddb_f, real_snd_offset, b"SND ")?;
            ddi_data.seek(SeekFrom::Start(ddi_snd_pos as u64))?;
            ddi_data.write_all(&ddb_snd_offset.to_le_bytes())?;
        }
    }
    println!("Writing DDI...");
    std::fs::write(&ddi_path, ddi_data.into_inner())?;
    println!("Finished...");
    Ok(())
}