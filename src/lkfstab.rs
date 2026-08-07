use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::exit;
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

#[derive(Debug, Clone, PartialEq)]
enum IdMode {
    Uuid,
    PartUuid,
    Label,
    Device,
}

struct MountEntry {
    spec: String,
    mountpoint: String,
    fstype: String,
    options: String,
    dump: u8,
    pass: u8,
}

const PSEUDO_FS: &[&str] = &[
    "sysfs", "proc", "devtmpfs", "devpts", "tmpfs", "cgroup", "cgroup2",
    "pstore", "bpf", "overlay", "squashfs", "iso9660", "ramfs", "autofs",
    "mqueue", "debugfs", "tracefs", "securityfs", "efivarfs", "configfs"
];

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut mode = IdMode::Uuid;
    let mut cmdline_mode = false;
    let mut target_dir: Option<String> = None;
    let mut idx = 1;
    while idx < args.len() {
        match args[idx].as_str() {
            "-u" | "--uuid" => mode = IdMode::Uuid,
            "-p" | "--partuuid" => mode = IdMode::PartUuid,
            "-l" | "--label" => mode = IdMode::Label,
            "-t" | "--device" => mode = IdMode::Device,
            "-b" | "--cmdline" => cmdline_mode = true,
            "-h" | "--help" => {
                print_help();
                exit(0);
            }
            arg => {
                if !arg.starts_with('-') {
                    target_dir = Some(arg.to_string());
                } else {
                    error(&format!("Option not recognized: {}", arg));
                    exit(1);
                }
            }
        }
        idx += 1;
    }
    require_root();
    let root_target = match target_dir {
        Some(dir) => canonicalize_path(&dir),
        None => {
            error("Target directory not specified! Use -h for help.");
            exit(1);
        }
    };
    info("Starting the operation....");
    let id_map_uuid = build_symlink_map("/dev/disk/by-uuid");
    let id_map_partuuid = build_symlink_map("/dev/disk/by-partuuid");
    let id_map_label = build_symlink_map("/dev/disk/by-label");
    let mut entries = parse_mounts(&root_target, &mode, &id_map_uuid, &id_map_partuuid, &id_map_label);
    let swap_entries = parse_swaps(&root_target, &mode, &id_map_uuid, &id_map_partuuid, &id_map_label);
    entries.extend(swap_entries);
    if cmdline_mode {
        generate_bootloader_cmdline(&entries);
    } else {
        generate_fstab(&entries);
    }
    success("Operation completed successfully!");
}

fn print_help() {
    println!("");
    println!("----------------------------------");
    println!("::: [ Liska Fstab (v1.0.0-1) ] :::");
    println!("----------------------------------");
    println!("");
    println!("Usage: lkfstab <command> [target_directory]");
    println!("> -u | --uuid         use UUID for partition identification (default)");
    println!("> -p | --partuuid     use PARTUUID for partition identification");
    println!("> -l | --label        use volume label for partition identification");
    println!("> -t | --device       use /dev/partition device node paths directly");
    println!("> -b | --cmdline      generate kernel bootloader parameters (root=UUID=...)");
    println!("");
}

fn canonicalize_path(p: &str) -> PathBuf {
    fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p))
}

fn build_symlink_map(dir: &str) -> HashMap<PathBuf, String> {
    let mut map = HashMap::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(real_path) = fs::canonicalize(&path) {
                if let Some(name) = path.file_name() {
                    map.insert(real_path, name.to_string_lossy().to_string());
                }
            }
        }
    }
    map
}

fn resolve_device_id(
    dev_path: &Path,
    mode: &IdMode,
    uuids: &HashMap<PathBuf, String>,
    partuuids: &HashMap<PathBuf, String>,
    labels: &HashMap<PathBuf, String>,
) -> String {
    let real_dev = fs::canonicalize(dev_path).unwrap_or_else(|_| dev_path.to_path_buf());
    match mode {
        IdMode::Uuid => {
            if let Some(uuid) = uuids.get(&real_dev) {
                format!("UUID={}", uuid)
            } else {
                real_dev.to_string_lossy().to_string()
            }
        }
        IdMode::PartUuid => {
            if let Some(partuuid) = partuuids.get(&real_dev) {
                format!("PARTUUID={}", partuuid)
            } else {
                real_dev.to_string_lossy().to_string()
            }
        }
        IdMode::Label => {
            if let Some(label) = labels.get(&real_dev) {
                format!("LABEL={}", label)
            } else {
                real_dev.to_string_lossy().to_string()
            }
        }
        IdMode::Device => real_dev.to_string_lossy().to_string(),
    }
}

fn parse_mounts(
    root_target: &Path,
    mode: &IdMode,
    uuids: &HashMap<PathBuf, String>,
    partuuids: &HashMap<PathBuf, String>,
    labels: &HashMap<PathBuf, String>,
) -> Vec<MountEntry> {
    let mut result = Vec::new();
    let file = match fs::File::open("/proc/mounts") {
        Ok(f) => f,
        Err(_) => return result,
    };
    let reader = BufReader::new(file);
    for line in reader.lines().flatten() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let spec_raw = parts[0];
        let mnt_raw = parts[1];
        let fstype = parts[2];
        let options = parts[3];
        if PSEUDO_FS.contains(&fstype) || spec_raw.starts_with("none") {
            continue;
        }
        let mnt_path = PathBuf::from(mnt_raw);
        if !mnt_path.starts_with(root_target) {
            continue;
        }
        let relative_mnt = match mnt_path.strip_prefix(root_target) {
            Ok(p) => {
                let s = p.to_string_lossy();
                if s.is_empty() {
                    "/".to_string()
                } else {
                    format!("/{}", s)
                }
            }
            Err(_) => continue,
        };
        let dev_node = PathBuf::from(spec_raw);
        let spec_resolved = resolve_device_id(&dev_node, mode, uuids, partuuids, labels);
        let (dump, pass) = if relative_mnt == "/" {
            (0, 1)
        } else if fstype == "vfat" || fstype == "fat" || fstype == "ext4" || fstype == "xfs" || fstype == "btrfs" {
            (0, 2)
        } else {
            (0, 0)
        };
        result.push(MountEntry {
            spec: spec_resolved,
            mountpoint: relative_mnt,
            fstype: fstype.to_string(),
            options: sanitize_options(options),
            dump,
            pass,
        });
    }
    result.sort_by(|a, b| a.mountpoint.cmp(&b.mountpoint));
    result
}

fn parse_swaps(
    root_target: &Path,
    mode: &IdMode,
    uuids: &HashMap<PathBuf, String>,
    partuuids: &HashMap<PathBuf, String>,
    labels: &HashMap<PathBuf, String>,
) -> Vec<MountEntry> {
    let mut result = Vec::new();
    let file = match fs::File::open("/proc/swaps") {
        Ok(f) => f,
        Err(_) => return result,
    };
    let reader = BufReader::new(file);
    for line in reader.lines().skip(1).flatten() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        let dev_raw = parts[0];
        let dev_node = PathBuf::from(dev_raw);
        if dev_raw.contains("file") && !dev_node.starts_with(root_target) {
            continue;
        }
        let spec_resolved = resolve_device_id(&dev_node, mode, uuids, partuuids, labels);
        result.push(MountEntry {
            spec: spec_resolved,
            mountpoint: "none".to_string(),
            fstype: "swap".to_string(),
            options: "defaults".to_string(),
            dump: 0,
            pass: 0,
        });
    }
    result
}

fn sanitize_options(opts: &str) -> String {
    let filter = ["rw", "ro", "nosuid", "nodev", "noexec", "relatime", "noatime", "subvol="];
    let mut clean_opts: Vec<&str> = opts
        .split(',')
        .filter(|o| filter.iter().any(|f| o.starts_with(f)))
        .collect();
    if clean_opts.is_empty() {
        return "defaults".to_string();
    }
    clean_opts.dedup();
    clean_opts.join(",")
}

fn generate_fstab(entries: &[MountEntry]) {
    println!("# /etc/fstab: Generated automatically by lkfstab (Liska Linux)");
    println!("# <file system>                            <mount point>  <type>  <options>                       <dump>  <pass>");
    for e in entries {
        println!(
            "{:<42} {:<14} {:<7} {:<31} {}       {}",
            e.spec, e.mountpoint, e.fstype, e.options, e.dump, e.pass
        );
    }
}

fn generate_bootloader_cmdline(entries: &[MountEntry]) {
    if let Some(root_entry) = entries.iter().find(|e| e.mountpoint == "/") {
        print!("root={} rw", root_entry.spec);
        if root_entry.fstype == "btrfs" {
            if let Some(subvol) = root_entry.options.split(',').find(|o| o.starts_with("subvol=")) {
                print!(" rootflags={}", subvol);
            }
        }
        println!();
    } else {
        error("Root partition not found under the target directory!");
        exit(1);
    }
}
