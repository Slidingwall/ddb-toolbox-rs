use std::fs::File;
use std::path::Path;
use anyhow::Result;
use crate::ddi::DDIModel;
pub fn main(src_path: &Path, out_dir: &Path, save_temp: bool, cat_only: bool) -> Result<()> {
    anyhow::ensure!(src_path.is_file(), "Source file does not exist: {}", src_path.display());
    std::fs::create_dir_all(&out_dir)?;
    let mut ddi = DDIModel::new(std::fs::read(src_path)?);
    println!("Loading DDI file...");
    let temp_path_for_ddi = if save_temp || cat_only { Some(out_dir) } else { None };
    ddi.read(temp_path_for_ddi, cat_only)?;
    println!("Saving DDI meta file...");
    serde_yaml::to_writer(File::create(out_dir.join("ddi.yml"))?, &ddi.ddi_data_dict)?;
    println!("Done.");
    Ok(())
}