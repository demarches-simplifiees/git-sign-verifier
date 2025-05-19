use std::fs;
use std::path::Path;
use std::process::Command;

pub fn extract_tar_archive(archive_path: &Path, dest_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dest_dir)?;

    let output = Command::new("tar")
        .current_dir(dest_dir)
        .args(&["xf", archive_path.to_str().unwrap()])
        .output()?;

    if !output.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "Failed to extract tar archive: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }

    Ok(())
}

pub fn copy_directory(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());

        if ty.is_dir() {
            copy_directory(&entry.path(), &dst_path)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), dst_path)?;
        }
    }
    Ok(())
}
