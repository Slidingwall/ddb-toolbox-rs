use serde::Serialize;
use std::fmt::Write;
#[derive(Debug, Serialize)]
pub struct ArticulationSegmentInfo {
    pub phonemes: Vec<String>,
    pub boundaries: Vec<f64>,
}
pub fn generate_transcription(seg_info: &[(String, f64, f64)]) -> String {
    let joined: String = seg_info.iter().map(|(p, _, _)| p.as_str()).collect::<Vec<_>>().join(" ");
    format!("{joined}\n[{joined}]")
}
pub fn generate_seg(
    phoneme_list: &[(String, f64, f64)],
    wav_length: f64,
    is_sta: bool,
) -> String {
    let mut buf = String::new();
    let sil = if is_sta { "unknown" } else { "Sil" };
    let sta_flag = if is_sta { 1 } else { 0 };
    let first = phoneme_list[0].1;
    let last = phoneme_list.last().unwrap().2;
    let _ = writeln!(buf, "nPhonemes {}", phoneme_list.len() + 2);
    let _ = writeln!(buf, "articulationsAreStationaries = {sta_flag}");
    let _ = writeln!(buf, "phoneme\t\tBeginTime\t\tEndTime");
    let _ = writeln!(buf, "===================================================");
    let _ = writeln!(buf, "{sil}\t\t0.000000\t\t{first:.6}");
    for (name, b, e) in phoneme_list {
        let _ = writeln!(buf, "{name}\t\t{b:.6}\t\t{e:.6}");
    }
    let _ = writeln!(buf, "{sil}\t\t{last:.6}\t\t{wav_length:.6}");
    buf
}
pub fn generate_articulation_seg(
    art: &ArticulationSegmentInfo,
    wav_samples: i32,
    unvoiced: &[String],
) -> String {
    use std::fmt::Write;
    let mut buf = String::new();
    let cut_len = (wav_samples as f64 / 2.0).floor() as i64;
    let is_tri = art.phonemes.len() == 3;
    let _ = writeln!(buf, "nphone art segmentation");
    let _ = writeln!(buf, "{{");
    let _ = writeln!(buf, "\tphns: [\"{}\"];", art.phonemes.join("\", \""));
    let _ = writeln!(buf, "\tcut offset: 0;");
    let _ = writeln!(buf, "\tcut length: {cut_len};");
    let bounds: Vec<String> = art.boundaries.iter().map(|x| format!("{x:.9}")).collect();
    let _ = writeln!(buf, "\tboundaries: [{}];", bounds.join(", "));
    let _ = writeln!(buf, "\trevised: false;");
    let mut voiced_bools = Vec::new();
    for (i, ph) in art.phonemes.iter().enumerate() {
        let is_voiced = !(unvoiced.contains(ph) || matches!(ph.as_str(), "Sil" | "Asp" | "?"));
        voiced_bools.push(is_voiced);
        if is_tri && i == 1 {
            voiced_bools.push(is_voiced);
        }
    }
    let voiced: Vec<String> = voiced_bools
        .iter()
        .map(|b| b.to_string().to_lowercase())
        .collect();
    let _ = writeln!(buf, "\tvoiced: [{}];", voiced.join(", "));
    let _ = writeln!(buf, "}};");
    let _ = writeln!(buf);
    buf
}