use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use colored::Colorize;

fn info(msg: &str) { println!("{} {}", "[i]".bright_cyan(), msg); }
fn success(msg: &str) { println!("{} {}", "[✓]".bright_green(), msg.bright_green()); }
fn error(msg: &str) { eprintln!("{} {}", "[✗]".bright_red(), msg.bright_red()); }

fn require_root() {
    if unsafe { libc::geteuid() } != 0 {
        error("Root permission required. Use 'sudo' for this operation.");
        exit(1);
    }
}

fn copy_internet_config(target: &Path) {
    info("Copying network configuration....");
    let host_resolv = Path::new("/etc/resolv.conf");
    let target_etc = target.join("etc");
    fs::create_dir_all(&target_etc).ok();
    let target_resolv = target_etc.join("resolv.conf");
    if host_resolv.exists() {
        if let Err(e) = fs::copy(host_resolv, &target_resolv) {
            error(&format!("Failed to copy network configuration: {}.", e));
        } else {
            success("Network configuration successfully copied!");
        }
    } else {
        let default_dns = "nameserver 1.1.1.1\nnameserver 8.8.8.8\n";
        fs::write(&target_resolv, default_dns).ok();
        info("Created default network configuration (1.1.1.1).");
    }
}

fn copy_os_release(target: &Path) {
    let host_os_release = Path::new("/etc/os-release");
    let target_etc = target.join("etc");
    fs::create_dir_all(&target_etc).ok();
    let target_os_release = target_etc.join("os-release");
    fs::copy(host_os_release, &target_os_release).ok();
}

fn setup_ca_certificates(target: &Path) {
    info("Generating CA certificates....");
    let target_str = target.to_str().unwrap_or(".");
    let status = Command::new("lkchroot")
        .args(&[target_str, "update-ca-trust"])
        .status();
    match status {
        Ok(s) if s.success() => {
            success("CA certificates has been generated successfully.");
        }
        _ => {
            error("Lkchroot update-ca-trust failed!");
            info("Copying host certificates....");
            let host_certs_dir = Path::new("/etc/ca-certificates/extracted");
            let target_certs_dir = target.join("etc/ca-certificates/extracted");
            if host_certs_dir.exists() {
                fs::create_dir_all(&target_certs_dir).ok();
                if let Ok(entries) = fs::read_dir(host_certs_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            let _ = fs::copy(&path, target_certs_dir.join(entry.file_name()));
                        }
                    }
                }
            }
            let target_ssl_dir = target.join("etc/ssl/certs");
            fs::create_dir_all(&target_ssl_dir).ok();
            let target_link = target_ssl_dir.join("ca-certificates.crt");
            let _ = fs::remove_file(&target_link);
            let _ = std::os::unix::fs::symlink(
                "/etc/ca-certificates/extracted/tls-ca-bundle.pem",
                &target_link
            );
            success("CA certificates setup completed.");
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 || args[1] == "--help" {
        println!("");
        println!("--------------------------------------");
        println!("::: [ Liska Bootstrap (v1.0.0-1) ] :::");
        println!("--------------------------------------");
        println!("");
        println!("Usage: lkstrap <target-directory> [packages]");
        println!("");
        exit(0);
    }
    require_root();
    let target_dir = PathBuf::from(&args[1]);
    let packages = &args[2..];
    info(&format!("Initializing bootstrap on {}", target_dir.display()));
    fs::create_dir_all(&target_dir).unwrap_or_else(|e| {
        error(&format!("Failed to create target directory: {}", e));
        exit(1);
    });
    info(&format!("Installing {} packages to {}....", packages.len(), target_dir.display()));
    let mut lkpm_args = vec![
        "-id".to_string(),
        format!("--root={}", target_dir.display()),
        "--noconfirm".to_string(),
    ];
    lkpm_args.extend_from_slice(packages);
    let status = Command::new("lkpm")
        .args(&lkpm_args)
        .status();
    match status {
        Ok(s) if s.success() => {
            success(&format!("Packages successfully installed to {}!", target_dir.display()));
            copy_internet_config(&target_dir);
            copy_os_release(&target_dir);
            setup_ca_certificates(&target_dir);
        }
        _ => {
            error("Failed to install base packages!");
            exit(1);
        }
    }
    success(&format!("Bootstrap completed successfully!"));
}
