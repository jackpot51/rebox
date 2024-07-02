use qemu::QEMU_X86_64_SOFTMMU;
use std::{env, error::Error, fs, process::Command};

mod progress_bar;
mod util;

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let cache_dir = dirs::cache_dir()
        .ok_or("user cache directory not found")?
        .join("rebox");
    println!("using cache directory {cache_dir:?}");
    fs::create_dir_all(&cache_dir)?;

    //TODO: allow recreating harddrive
    let hd_path = cache_dir.join("harddrive.img");
    if !hd_path.is_file() {
        let img_url = "https://static.redox-os.org/img/x86_64";
        let shasum_url = format!("{img_url}/SHA256SUM");
        let shasum = reqwest::blocking::get(shasum_url)?.text()?;
        let mut image_opt = None;
        for line in shasum.lines() {
            let sha256 = &line[..64];
            let name = &line[66..];
            if name.starts_with("redox_demo_x86_64_") && name.ends_with("_harddrive.img.zst") {
                image_opt = Some((name.to_string(), sha256.to_string()));
            }
        }

        let (image_name, image_sha256) = image_opt.ok_or("demo harddrive image not found")?;
        println!("downloading {image_name}");
        let image_url = format!("{img_url}/{image_name}");
        let image_path = cache_dir.join(image_name);
        util::sha256_or_download(&image_url, &image_sha256, &image_path)?;

        let hd_partial = cache_dir.join("harddrive.partial");
        util::zstd_decompress_progress(&image_path, &hd_partial)?;
        fs::rename(&hd_partial, &hd_path)?;
    }

    let qemu_url = "https://download.qemu.org/qemu-9.0.1.tar.xz";
    let qemu_sha256 = "d0f4db0fbd151c0cf16f84aeb2a500f6e95009732546f44dafab8d2049bbb805";
    //TODO: use sha256 to ensure directory is re-extracted as needed?
    let qemu_dir = cache_dir.join(format!("qemu"));
    if !qemu_dir.is_dir() {
        println!("downloading QEMU source");
        let qemu_tar_xz = cache_dir.join("qemu.tar.xz");
        util::sha256_or_download(qemu_url, qemu_sha256, &qemu_tar_xz)?;

        println!("extracting QEMU source");
        let qemu_partial = cache_dir.join(format!("qemu.partial"));
        if qemu_partial.is_dir() {
            //TODO: race conditions, use lockfile on cache directory
            fs::remove_dir_all(&qemu_partial)?;
        }
        util::extract_progress(&qemu_tar_xz, &qemu_partial)?;
        fs::rename(&qemu_partial, &qemu_dir)?;
    }

    let qemu_system_x86_64 = cache_dir.join("qemu-system-x86_64");
    if !qemu_system_x86_64.is_file() {
        println!("extracting QEMU binary");
        let qemu_system_x86_64_partial = cache_dir.join("qemu-system-x86_64");
        fs::write(&qemu_system_x86_64_partial, QEMU_X86_64_SOFTMMU)?;

        #[cfg(unix)]
        {
            println!("marking QEMU binary as read-only and executable");
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                &qemu_system_x86_64_partial,
                fs::Permissions::from_mode(0o555),
            )?;
        }

        fs::rename(&qemu_system_x86_64_partial, &qemu_system_x86_64)?;
    }

    let mut command = Command::new(qemu_system_x86_64);

    // Set window name
    command.arg("-name").arg("Redox OS x86_64");

    //TODO: kvm not always available
    let kvm = true;
    if kvm {
        command.arg("-enable-kvm").arg("-cpu").arg("host");
    } else {
        command.arg("-cpu").arg("max");
    }

    // Use q35 machine
    command.arg("-machine").arg("q35");

    // Redox needs 2 GiB of RAM
    command.arg("-m").arg("2048");

    // Use 4 CPUs
    //TODO: detect host CPUs?
    command.arg("-smp").arg("4");

    // Serial output
    command.arg("-serial").arg("stdio");

    // HDA audio device
    command.arg("-device").arg("ich9-intel-hda");
    command.arg("-device").arg("hda-output");

    // E1000 ethernet device
    command.arg("-netdev").arg("user,id=net0");
    command.arg("-device").arg("e1000,netdev=net0");

    // Downloaded QEMU BIOS
    command.arg("-L").arg(qemu_dir.join("qemu-9.0.1/pc-bios"));

    // Downloaded harddrive
    command
        .arg("-drive")
        .arg(format!("file={},format=raw", hd_path.display()));

    // Add any additional arguments from the command line
    command.args(env::args().skip(1));

    println!("running {:?}", command);
    command.spawn()?.wait()?;
    Ok(())
}
