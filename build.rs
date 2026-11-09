use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=assets/icons/windows/rustduck.ico");

    #[cfg(all(target_os = "windows", target_env = "msvc"))]
    {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
        if let Err(err) = compile_windows_icon_resource() {
            println!("cargo:warning=Failed to embed Windows icon resource: {err}");
        }
    }
}

#[cfg(all(target_os = "windows", target_env = "msvc"))]
fn compile_windows_icon_resource() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let ico_path = manifest_dir.join("assets/icons/windows/rustduck.ico");
    let ico = fs::read(&ico_path)?;
    if ico.len() < 6 {
        return Err("ICO file too small".into());
    }

    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    if ico.len() < 6 + count * 16 {
        return Err("ICO directory truncated".into());
    }

    let mut res = Vec::new();

    // 1. Standard 32-byte null resource header at start of .res file
    write_res_header(&mut res, 0, 0, 0, 0);

    // 2. Write each RT_ICON (Type = 3) entry with ordinal ID = 1..=count
    let mut grp_data = Vec::with_capacity(6 + count * 14);
    grp_data.extend_from_slice(&[0, 0, 1, 0]);
    grp_data.extend_from_slice(&(count as u16).to_le_bytes());

    for i in 0..count {
        let entry_offset = 6 + i * 16;
        let entry = &ico[entry_offset..entry_offset + 16];
        let bytes_in_res = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
        let img_offset = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;

        if img_offset + bytes_in_res > ico.len() {
            return Err("ICO image data out of bounds".into());
        }

        let icon_id = (i + 1) as u16;
        // RT_ICON = 3
        write_res_header(&mut res, bytes_in_res as u32, 3, icon_id, 0x1010);
        res.extend_from_slice(&ico[img_offset..img_offset + bytes_in_res]);
        pad_dword(&mut res);

        // GRPICONDIRENTRY: first 12 bytes of ICONDIRENTRY + 2-byte icon_id
        grp_data.extend_from_slice(&entry[0..12]);
        grp_data.extend_from_slice(&icon_id.to_le_bytes());
    }

    // 3. Write RT_GROUP_ICON (Type = 14) with ordinal ID = 1 (loaded by GPUI LoadImageW(PCWSTR(1)))
    write_res_header(&mut res, grp_data.len() as u32, 14, 1, 0x1030);
    res.extend_from_slice(&grp_data);
    pad_dword(&mut res);

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let res_path = out_dir.join("rustduck_icon.res");
    fs::write(&res_path, &res)?;

    println!("cargo:rustc-link-arg-bins={}", res_path.display());
    Ok(())
}

#[cfg(all(target_os = "windows", target_env = "msvc"))]
fn write_res_header(buf: &mut Vec<u8>, data_size: u32, res_type: u16, res_id: u16, memory_flags: u16) {
    buf.extend_from_slice(&data_size.to_le_bytes());
    buf.extend_from_slice(&32u32.to_le_bytes()); // HeaderSize = 32 bytes
    buf.extend_from_slice(&0xFFFFu16.to_le_bytes());
    buf.extend_from_slice(&res_type.to_le_bytes());
    buf.extend_from_slice(&0xFFFFu16.to_le_bytes());
    buf.extend_from_slice(&res_id.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
    buf.extend_from_slice(&memory_flags.to_le_bytes());
    buf.extend_from_slice(&0x0409u16.to_le_bytes()); // LanguageId (en-US)
    buf.extend_from_slice(&0u32.to_le_bytes()); // Version
    buf.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
}

#[cfg(all(target_os = "windows", target_env = "msvc"))]
fn pad_dword(buf: &mut Vec<u8>) {
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
}
