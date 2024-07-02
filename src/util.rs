use pbr::{self, ProgressBar};
use reqwest;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Error, ErrorKind, Read, Result, Write};
use std::path::Path;
use std::time::Duration;

use crate::progress_bar::{ProgressBarRead, ProgressBarWrite};

pub fn download_length(url: &str) -> reqwest::Result<Option<u64>> {
    let client = reqwest::blocking::Client::new();

    let resp = client.head(url).send()?.error_for_status()?;

    Ok(resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|len| len.to_str().ok())
        .and_then(|len| len.parse().ok()))
}

pub fn download<W: Write>(url: &str, w: &mut W) -> reqwest::Result<u64> {
    let mut resp = reqwest::blocking::get(url)?.error_for_status()?;
    resp.copy_to(w)
}

pub fn download_progress<P: AsRef<Path>>(url: &str, path: P) -> Result<u64> {
    let len = download_length(url)
        .map_err(|err| Error::new(ErrorKind::Other, err))?
        .ok_or(Error::new(ErrorKind::Other, "ContentLength not found"))?;

    let mut f = fs::File::create(&path)?;

    let mut pb = ProgressBar::new(len);
    pb.message("download: ");
    pb.set_max_refresh_rate(Some(Duration::new(1, 0)));
    pb.set_units(pbr::Units::Bytes);

    let res = {
        let mut pbw = ProgressBarWrite::new(&mut pb, &mut f);
        download(url, &mut pbw).map_err(|err| Error::new(ErrorKind::Other, err))
    };

    pb.finish_println("");

    f.sync_all()?;

    res
}

pub fn extract<R: Read, P: AsRef<Path>>(r: &mut R, dst: P) -> Result<()> {
    let xz = xz2::read::XzDecoder::new(r);
    let mut tar = tar::Archive::new(xz);
    tar.unpack(dst)?;
    Ok(())
}

pub fn extract_progress<P: AsRef<Path>, Q: AsRef<Path>>(src: P, dst: Q) -> Result<()> {
    let len = fs::metadata(&src)?.len();

    let mut r = fs::File::open(&src)?;

    let mut pb = ProgressBar::new(len);
    pb.message("extract: ");
    pb.set_max_refresh_rate(Some(Duration::new(1, 0)));
    pb.set_units(pbr::Units::Bytes);

    let res = {
        let mut pbr = ProgressBarRead::new(&mut pb, &mut r);
        extract(&mut pbr, dst)
    };

    pb.finish_println("");

    res
}

pub fn sha256<R: Read>(r: &mut R) -> Result<String> {
    let mut hasher = Sha256::new();

    let mut data = vec![0; 4 * 1024 * 1024];
    loop {
        let count = r.read(&mut data)?;
        if count == 0 {
            break;
        }

        hasher.update(&data[..count]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

pub fn sha256_progress<P: AsRef<Path>>(path: P) -> Result<String> {
    let len = fs::metadata(&path)?.len();

    let mut f = fs::File::open(&path)?;

    let mut pb = ProgressBar::new(len);
    pb.message("verify: ");
    pb.set_max_refresh_rate(Some(Duration::new(1, 0)));
    pb.set_units(pbr::Units::Bytes);

    let res = {
        let mut pbr = ProgressBarRead::new(&mut pb, &mut f);
        sha256(&mut pbr)
    };

    pb.finish_println("");

    res
}

pub fn sha256_or_download<P: AsRef<Path>>(url: &str, sha256: &str, path: P) -> Result<()> {
    let path = path.as_ref();
    if path.exists() {
        let path_sha256 = sha256_progress(&path)?;
        if path_sha256 == sha256 {
            // File already exists and matches hash
            return Ok(());
        } else {
            log::warn!("previous file at {path:?} has hash {path_sha256:?} instead of {sha256:?}");
            // Remove file that does not match hash
            fs::remove_file(&path)?;
        }
    }

    // Download file
    download_progress(url, &path)?;
    let path_sha256 = sha256_progress(&path)?;
    if path_sha256 == sha256 {
        // Downloaded file matches hash
        Ok(())
    } else {
        let message = format!("downloaded file from {url:?} to {path:?} has hash {path_sha256:?} instead of {sha256:?}");
        log::error!("{}", message);
        // Remove file that does not match hash
        fs::remove_file(&path)?;
        Err(Error::new(ErrorKind::InvalidData, message))
    }
}

pub fn zstd_decompress<R: Read, W: Write>(r: &mut R, w: &mut W) -> Result<()> {
    zstd::stream::copy_decode(r, w)?;
    Ok(())
}

pub fn zstd_decompress_progress<P: AsRef<Path>, Q: AsRef<Path>>(input: P, output: Q) -> Result<()> {
    let len = fs::metadata(&input)?.len();

    let mut r = fs::File::open(&input)?;
    let mut w = fs::File::create(&output)?;

    let mut pb = ProgressBar::new(len);
    pb.message("decompress: ");
    pb.set_max_refresh_rate(Some(Duration::new(1, 0)));
    pb.set_units(pbr::Units::Bytes);

    let res = {
        let mut pbr = ProgressBarRead::new(&mut pb, &mut r);
        zstd_decompress(&mut pbr, &mut w)
    };

    pb.finish_println("");

    w.sync_all()?;

    res
}
