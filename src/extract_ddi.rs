use std::fs::File;
use std::path::Path;
use anyhow::Result;
use crate::ddi::DDIModel;
pub fn main(src_path: &Path, save_temp: bool, cat_only: bool) -> Result<()> {
    anyhow::ensure!(src_path.is_file(), "Source file does not exist: {}", src_path.display());
    let dst_path = src_path.parent().unwrap_or(Path::new(".")).join(src_path.file_stem().unwrap());
    std::fs::create_dir_all(&dst_path)?;
    let mut ddi = DDIModel::new(std::fs::read(src_path)?);
    println!("Loading DDI file...");
    match (save_temp, cat_only) {
        (true, _) | (_, true) => ddi.read(Some(&dst_path), cat_only)?,
        _ => ddi.read(None, false)?,
    }
    println!("Saving DDI meta file...");
    serde_json::to_writer_pretty(File::create(dst_path.join("ddi.yml"))?, &ddi.ddi_data_dict)?;
    println!("Done.");
    Ok(())
}