use std::collections::HashSet;
use std::fs::{write, File, create_dir_all};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use anyhow::Result;
use byteorder::{LittleEndian, ReadBytesExt};
use hound::{WavSpec, WavWriter};
use crate::ddi::{stream_reverse_search, DDIModel};
use crate::seg::{
    generate_articulation_seg, generate_seg, generate_transcription, ArticulationSegmentInfo,
};
const START_ENCODE: &[u8] = b"SND ";
fn escape_xsampa(xsampa: &str) -> String {
    xsampa
        .replace("Sil", "sil")
        .replace('\\', "-")
        .replace('/', "~")
        .replace('?', "!")
        .replace(':', ";")
        .replace('<', "(")
        .replace('>', ")")
        .replace('*', "•")
}
fn create_file_name(
    phonemes: &[String],
    classify: bool,
    offset: u64,
    pitch: f32,
    ty: &str,
    idx: Option<usize>,
) -> String {
    let offset_hex = format!("{offset:08x}");
    let escaped: Vec<_> = phonemes.iter().map(|p| escape_xsampa(p)).collect();
    let group = match phonemes.len() {
        1 if phonemes[0] == "growl" => "growl",
        1 => "sta",
        2 => "art",
        3 => "tri",
        _ => return format!("unknown_{offset_hex}.{ty}"),
    };
    let prefix = if ty == "lab" { "lab" } else { "wav" };
    let dir = match (classify, idx) {
        (true, Some(i)) => format!("{group}/{}/{prefix}", i + 1),
        _ => format!("{group}/{prefix}"),
    };
    format!(
        "{dir}/[{}]_pit{:+0.2}_{offset_hex}.{ty}",
        escaped.join(" "),
        pitch
    )
}
#[inline(always)]
fn nsample2sec(nsample: usize, sample_rate: u32) -> f64 {
    nsample as f64 / sample_rate as f64 / 2.0
}
#[inline(always)]
fn frm2sec(frm: u32, sample_rate: u32) -> f64 {
    frm as f64 * 512.0 / sample_rate as f64 / 2.0
}
fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            create_dir_all(parent)?;
        }
    }
    Ok(())
}
fn write_snd_to_wav(
    mut file: impl Read,
    path: &Path,
    ch: u16,
    sr: u32,
    len: usize,
) -> Result<()> {
    ensure_parent_dir(path)?;
    let spec = WavSpec {
        channels: ch,
        sample_rate: sr,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec)?;
    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf)?;
    buf.chunks_exact(2).for_each(|c| {
        let sample = i16::from_le_bytes(c.try_into().unwrap());
        writer.write_sample(sample).unwrap();
    });
    writer.finalize()?;
    Ok(())
}
pub fn main(
    ddi_path: &Path,
    dst_path: PathBuf,
    gen_lab: bool,
    gen_seg: bool,
    classify: bool,
) -> Result<()> {
    let ddb_path = ddi_path.with_extension("ddb");
    std::fs::create_dir_all(&dst_path)?;
    let mut dumped = HashSet::new();
    let mut ddb_file = File::open(&ddb_path)?;
    let ddb_size = std::fs::metadata(&ddb_path)?.len() as f64;
    let mut ddi = DDIModel::new(std::fs::read(ddi_path)?);
    ddi.read(None, false)?;
    let mut art_list = Vec::new();
    for item in ddi.art_data.values() {
        let Some(ph1) = item.get("phoneme").and_then(|v| v.as_str()) else {
            continue;
        };
        if let Some(artu) = item.get("artu").and_then(|v| v.as_mapping()) {
            for a in artu.values() {
                let Some(ph2) = a.get("phoneme").and_then(|v| v.as_str()) else {
                    continue;
                };
                let phns = vec![ph1.to_string(), ph2.to_string()];
                if let Some(artp) = a.get("artp").and_then(|v| v.as_mapping()) {
                    art_list.extend(
                        artp.iter()
                            .enumerate()
                            .map(|(i, p)| (i, phns.clone(), p.1))
                    );
                }
            }
        }
        if let Some(art) = item.get("art").and_then(|v| v.as_mapping()) {
            for s in art.values() {
                let Some(ph2) = s.get("phoneme").and_then(|v| v.as_str()) else {
                    continue;
                };
                if let Some(artu) = s.get("artu").and_then(|v| v.as_mapping()) {
                    for a in artu.values() {
                        let Some(ph3) = a.get("phoneme").and_then(|v| v.as_str()) else {
                            continue;
                        };
                        let phns = vec![
                            ph1.to_string(),
                            ph2.to_string(),
                            ph3.to_string(),
                        ];
                        if let Some(artp) = a.get("artp").and_then(|v| v.as_mapping()) {
                            art_list.extend(
                                artp.values()
                                    .map(|p| (0, phns.clone(), p))
                            );
                        }
                    }
                }
            }
        }
    }
    for (idx, phns, art) in art_list {
        let Some(snd_str) = art.get("snd").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some((_, snd_part)) = snd_str.split_once('=') else {
            continue;
        };
        let Some((snd_off_hex, _)) = snd_part.split_once('_') else {
            continue;
        };
        let Ok(snd_off) = usize::from_str_radix(snd_off_hex, 16) else {
            continue;
        };
        let Some(pitch) = art.get("pitch1").and_then(|v| v.as_f64()) else {
            continue;
        };
        let pitch = pitch as f32;
        let path = dst_path.join(create_file_name(&phns, classify, snd_off as u64, pitch, "wav", Some(idx)));
        ddb_file.seek(SeekFrom::Start(snd_off as u64))?;
        let mut sig = [0u8; 4];
        if ddb_file.read_exact(&mut sig).is_err() || sig != START_ENCODE {
            continue;
        }
        let len = ddb_file.read_u32::<LittleEndian>()? as usize;
        let (sr, ch) = (ddb_file.read_u32::<LittleEndian>()?, ddb_file.read_u16::<LittleEndian>()?);
        ddb_file.seek(SeekFrom::Current(4))?;
        write_snd_to_wav(&mut ddb_file, &path, ch, sr, len - 18)?;
        dumped.insert(snd_off);
        println!("Dumped [{}] -> {}", phns.join(" "), path.display());
        if (gen_lab || gen_seg) && art.get("frame_align").is_some() {
            let Some(fa) = art.get("frame_align").and_then(|v| v.as_sequence()) else {
                continue;
            };
            let Some(snd_start_str) = art.get("snd_start").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some((_, snd_start_part)) = snd_start_str.split_once('=') else {
                continue;
            };
            let Some((snd_vstart_hex, _)) = snd_start_part.split_once('_') else {
                continue;
            };
            let Ok(snd_vstart) = usize::from_str_radix(snd_vstart_hex, 16) else {
                continue;
            };
            let empty_bytes = snd_vstart - snd_off;
            if gen_lab {
                let offset_t = nsample2sec(empty_bytes, sr) * 1e7;
                let dur_t = nsample2sec(len, sr) * 1e7;
                let mut lines = vec![format!("0 {:.0} sil", offset_t)];
                let phns_lab = if phns.len() == 3 {
                    let c = phns[1].replacen('^', "", 1);
                    vec![phns[0].clone(), c.clone(), c, phns[2].clone()]
                } else {
                    phns.clone()
                };
                let mut last = 0.0;
                for (i, p) in phns_lab.iter().enumerate() {
                    let Some(frame) = fa.get(i) else {
                        break;
                    };
                    let Some(start) = frame.get("start").and_then(|v| v.as_u64()) else {
                        continue;
                    };
                    let Some(end) = frame.get("end").and_then(|v| v.as_u64()) else {
                        continue;
                    };
                    let s = frm2sec(start as u32, sr) * 1e7 + offset_t;
                    let e = frm2sec(end as u32, sr) * 1e7 + offset_t;
                    lines.push(format!("{s:.0} {e:.0} {p}"));
                    last = e;
                }
                lines.push(format!("{last:.0} {dur_t:.0} sil"));
                let lab_path = dst_path.join(create_file_name(&phns_lab, classify, snd_off as u64, pitch, "lab", Some(idx)));
                ensure_parent_dir(&lab_path)?;
                write(lab_path, lines.join("\n"))?;
            }
            if gen_seg {
                let offset_t = nsample2sec(empty_bytes, sr);
                let dur_t = nsample2sec(len, sr);
                let phns_seg = if phns.len() == 3 {
                    vec![phns[0].clone(), phns[1].replacen('^', "", 1), phns[2].clone()]
                } else {
                    phns.clone()
                };
                let mut bounds = Vec::new();
                let chunks = if phns_seg.len() == 3 { 4 } else { 2 };
                for i in 0..chunks {
                    let Some(frame) = fa.get(i) else {
                        break;
                    };
                    let Some(start) = frame.get("start").and_then(|v| v.as_u64()) else {
                        continue;
                    };
                    let Some(end) = frame.get("end").and_then(|v| v.as_u64()) else {
                        continue;
                    };
                    let s = offset_t + frm2sec(start as u32, sr);
                    let e = offset_t + frm2sec(end as u32, sr);
                    if i == 0 { bounds.push(s); }
                    bounds.push(e);
                }
                let seg: Vec<(String, f64, f64)> = match phns_seg.len() {
                    3 => {
                        if bounds.len() >= 5 {
                            vec![
                                (phns_seg[0].clone(), bounds[0], bounds[1]),
                                (phns_seg[1].clone(), bounds[1], bounds[3]),
                                (phns_seg[2].clone(), bounds[3], bounds[4]),
                            ]
                        } else {
                            continue;
                        }
                    },
                    _ => {
                        if bounds.len() >= 3 {
                            vec![
                                (phns_seg[0].clone(), bounds[0], bounds[1]),
                                (phns_seg[1].clone(), bounds[1], bounds[2]),
                            ]
                        } else {
                            continue;
                        }
                    },
                };
                let unvoiced: Vec<String> = ddi.phdc_data.get("phoneme")
                    .and_then(|v| v.get("unvoiced"))
                    .and_then(|v| v.as_sequence())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let (trans, seg_str, as0) = (
                    generate_transcription(&seg),
                    generate_seg(&seg, dur_t, false),
                    generate_articulation_seg(&ArticulationSegmentInfo { phonemes: phns_seg, boundaries: bounds }, len as i32, &unvoiced)
                );
                let phonemes_for_file: Vec<String> = seg.iter().map(|x| x.0.clone()).collect();
                let trans_path = dst_path.join(create_file_name(&phonemes_for_file, classify, snd_off as u64, pitch, "trans", Some(idx)));
                let seg_path = dst_path.join(create_file_name(&phonemes_for_file, classify, snd_off as u64, pitch, "seg", Some(idx)));
                let as0_path = dst_path.join(create_file_name(&phonemes_for_file, classify, snd_off as u64, pitch, "as0", Some(idx)));
                ensure_parent_dir(&trans_path)?;
                ensure_parent_dir(&seg_path)?;
                ensure_parent_dir(&as0_path)?;
                write(trans_path, trans)?;
                write(seg_path, seg_str)?;
                write(as0_path, as0)?;
            }
        }
    }
    for sta in ddi.sta_data.values() {
        let Some(phoneme) = sta.get("phoneme").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(stap) = sta.get("stap").and_then(|v| v.as_mapping()) else {
            continue;
        };
        for (i, item) in stap.iter().enumerate() {
            let Some(snd_str) = item.1.get("snd").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some((_, snd_part)) = snd_str.split_once('=') else {
                continue;
            };
            let Some((snd_off_hex, _)) = snd_part.split_once('_') else {
                continue;
            };
            let Ok(snd_off) = usize::from_str_radix(snd_off_hex, 16) else {
                continue;
            };
            let Some(pitch) = item.1.get("pitch1").and_then(|v| v.as_f64()) else {
                continue;
            };
            let pitch = pitch as f32;
            let phns = vec![phoneme.to_string()];
            let path = dst_path.join(create_file_name(&phns, classify, snd_off as u64, pitch, "wav", Some(i)));
            let real_off = stream_reverse_search(&mut ddb_file, START_ENCODE, snd_off, 32768);
            if real_off == usize::MAX {
                continue;
            }
            ddb_file.seek(SeekFrom::Start(real_off as u64 + 4))?;
            let len = ddb_file.read_u32::<LittleEndian>()? as usize;
            let (sr, ch) = (ddb_file.read_u32::<LittleEndian>()?, ddb_file.read_u16::<LittleEndian>()?);
            ddb_file.seek(SeekFrom::Current(4))?;
            write_snd_to_wav(&mut ddb_file, &path, ch, sr, len - 18)?;
            dumped.insert(real_off);
            println!("Dumped [{phoneme}] -> {}", path.display());
            if gen_lab || gen_seg {
                let offset_pos = snd_off - real_off;
                let Some(snd_len_val) = item.1.get("snd_length").and_then(|v| v.as_u64()) else {
                    continue;
                };
                let cutoff_pos = snd_len_val as usize - offset_pos;
                if gen_lab {
                    let (o, c, d) = (
                        nsample2sec(offset_pos, sr) * 1e7,
                        nsample2sec(cutoff_pos, sr) * 1e7,
                        nsample2sec(len, sr) * 1e7,
                    );
                    let content = format!("0 {o:.0} sil\n{o:.0} {c:.0} {phoneme}\n{c:.0} {d:.0} sil");
                    let lab_path = dst_path.join(create_file_name(&phns, classify, snd_off as u64, pitch, "lab", Some(i)));
                    ensure_parent_dir(&lab_path)?;
                    write(lab_path, content)?;
                }
                if gen_seg {
                    let seg = vec![(phoneme.to_string(), nsample2sec(offset_pos, sr), nsample2sec(cutoff_pos, sr))];
                    let (trans, seg_str) = (generate_transcription(&seg), generate_seg(&seg, nsample2sec(len, sr), true));
                    let trans_path = dst_path.join(create_file_name(&phns, classify, snd_off as u64, pitch, "trans", Some(i)));
                    let seg_path = dst_path.join(create_file_name(&phns, classify, snd_off as u64, pitch, "seg", Some(i)));
                    ensure_parent_dir(&trans_path)?;
                    ensure_parent_dir(&seg_path)?;
                    write(trans_path, trans)?;
                    write(seg_path, seg_str)?;
                }
            }
        }
    }
    if let Some(vqm) = ddi.vqm_data.as_ref() {
        for (idx, item) in vqm.iter() {
            let Some(snd_str) = item.get("snd").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some((_, snd_part)) = snd_str.split_once('=') else {
                continue;
            };
            let Some((snd_off_hex, _)) = snd_part.split_once('_') else {
                continue;
            };
            let Ok(snd_off) = usize::from_str_radix(snd_off_hex, 16) else {
                continue;
            };
            let Some(pitch) = item.get("pitch1").and_then(|v| v.as_f64()) else {
                continue;
            };
            let pitch = pitch as f32;
            let phns = vec!["growl".to_string()];
            let path = dst_path.join(create_file_name(&phns, classify, snd_off as u64, pitch, "wav", None));
            ddb_file.seek(SeekFrom::Start(snd_off as u64))?;
            let mut sig = [0u8; 4];
            if ddb_file.read_exact(&mut sig).is_err() || sig != START_ENCODE {
                eprintln!("Error: SND header not found for VQM {idx}");
                continue;
            }
            let len = ddb_file.read_u32::<LittleEndian>()? as usize;
            let (sr, ch) = (ddb_file.read_u32::<LittleEndian>()?, ddb_file.read_u16::<LittleEndian>()?);
            ddb_file.seek(SeekFrom::Current(4))?;
            write_snd_to_wav(&mut ddb_file, &path, ch, sr, len - 18)?;
            dumped.insert(snd_off);
            println!("Dumped VQM growl -> {}", path.display());
        }
    }
    println!("Scan for unindexed SND...");
    let mut progress_time = std::time::Instant::now();
    ddb_file.seek(SeekFrom::Start(0))?;
    let mut buf = [0u8; 10240];
    loop {
        let pos = ddb_file.stream_position()?;
        let n = ddb_file.read(&mut buf)?;
        if n == 0 { break; }
        for i in 0..n {
            if i + 4 > n || &buf[i..i+4] != START_ENCODE { continue; }
            let snd_off = (pos + i as u64) as usize;
            if dumped.contains(&snd_off) {
                println!("Skip dumped SND -> {}", dst_path.join(create_file_name(&[], classify, snd_off as u64, 0.0, "wav", None)).display());
                continue;
            }
            ddb_file.seek(SeekFrom::Start(snd_off as u64 + 4))?;
            let len = ddb_file.read_u32::<LittleEndian>()? as usize;
            let (sr, ch) = (ddb_file.read_u32::<LittleEndian>()?, ddb_file.read_u16::<LittleEndian>()?);
            ddb_file.seek(SeekFrom::Current(4))?;
            let path = dst_path.join(create_file_name(&[], classify, snd_off as u64, 0.0, "wav", None));
            write_snd_to_wav(&mut ddb_file, &path, ch, sr, len - 18)?;
            dumped.insert(snd_off);
            println!("Dumped unindexed SND -> {}", path.display());
            ddb_file.seek(SeekFrom::Start(pos + i as u64 + 4))?;
        }
        if progress_time.elapsed().as_secs_f64() > 0.5 {
            print!("Progress: {:.2}%\r", (pos as f64 / ddb_size) * 100.0);
            progress_time = std::time::Instant::now();
        }
        if n < 10240 { break; }
        ddb_file.seek(SeekFrom::Start(pos + 10240 - 4))?;
    }
    println!("Progress: 100.00%\nDone");
    Ok(())
}