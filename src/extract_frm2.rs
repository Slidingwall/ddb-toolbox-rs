use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use anyhow::Result;
use zip::{CompressionMethod, write::FileOptions};
use memchr::memmem;
pub fn main(src_path: &Path, dst_path: Option<&Path>) -> Result<()> {
    let dst_path = dst_path.map(PathBuf::from).unwrap_or_else(|| {
        src_path.parent().unwrap_or(Path::new("."))
            .join(src_path.file_stem().unwrap())
            .join("frm2.zip")
    });
    anyhow::ensure!(
        dst_path.extension().unwrap_or_default() == "zip",
        "Destination must be a .zip file"
    );
    if let Some(dir) = dst_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut ddb_data = Vec::new();
    File::open(src_path)?.read_to_end(&mut ddb_data)?;
    let length = ddb_data.len();
    let mut zip = zip::ZipWriter::new(File::create(&dst_path)?);
    let zip_opts: FileOptions<'_, ()> = FileOptions::default().compression_method(CompressionMethod::Stored);
    let mut counter = 0;
    let mut offset = 0;
    while let Some(start_idx) = memmem::find(&ddb_data[offset..], b"FRM2") {
        let start_idx = offset + start_idx;
        let file_length = u32::from_le_bytes(ddb_data[start_idx+4..start_idx+8].try_into()?) as usize;
        offset = start_idx + file_length;
        if offset > length {
            break;
        }
        let frm2_data = &ddb_data[start_idx..offset];
        counter += 1;
        println!("{counter:<10} progress: {offset:08x} / {length:08x}");
        let file_path = format!("frm2/{start_idx:08x}.frm2");
        zip.start_file(&file_path, zip_opts)?;
        zip.write_all(frm2_data)?;
        println!("    frm2 saved at: {file_path}");
    }
    zip.finish()?;
    println!("zip file saved at: {}", dst_path.display());
    Ok(())
}