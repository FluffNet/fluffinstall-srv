use std::fs::File;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};

const VERSION: &str = "0.9";

const BOLD_RED: &str = "\x1b[1;31m";
const BOLD_GREEN: &str = "\x1b[1;32m";
const YELLOW: &str = "\x1b[33m";
const PURPLE: &str = "\x1b[35m";
const RESET: &str = "\x1b[0m";

const TABLE_INDENT: &str = "  ";
const MIN_STORAGE_GIB: u64 = 10;

const HOSTNAME_REQUIREMENTS: &str = "Hostname Requirements:
Allowed characters: letters (a-z and A-Z), digits (0-9), dash (-), and dot (.).
Cannot start or end with a dash (-) or a dot (.).
Special characters are not allowed (for example: @ and !)
No spaces allowed.
Max length: 255 characters.";

const USERNAME_REQUIREMENTS: &str = "User Name Requirements:
Only lowercase letters (a-z), digits (0-9), underscore (_), or dash (-).
No spaces allowed.
Cannot start or end with a dash (-) or an underscore (_)
Special characters are not allowed (for example: @ and !)
Must start with a lowercase letter; cannot be only numbers
Max length: 32 characters";

// NOTE:
// All packages and their dependencies MUST exist in the offline repository
// located at /usr/lib/fluffinstall/packages/ (as defined in /etc/pacman.d/fluffinstall.conf).
// If any package or dependency is missing, pacstrap WILL fail.
const PACKAGE_LIST: &str = "base archlinux-keyring flufflinuxsrv-filesystem linux-lts linux-firmware linux-firmware-marvell linux-firmware-bnx2x amd-ucode intel-ucode arch-install-scripts b43-fwcutter cryptsetup ddrescue diffutils dmidecode dmraid dosfstools e2fsprogs edk2-shell efibootmgr grub ethtool exfatprogs fatresize fsarchiver gpart gptfdisk hdparm less libusb-compat lsscsi lvm2 man-db man-pages mdadm memtest86+-efi mkinitcpio modemmanager mtools nano nfs-utils nmap ntfs-3g nvme-cli open-iscsi openssh partclone parted networkmanager networkmanager-openvpn pv rp-pppoe rsync sdparm sg3_utils smartmontools squashfs-tools sudo systemd-resolvconf tcpdump testdisk tmux tpm2-tools tpm2-tss udftools usb_modeswitch usbutils vim wireless-regdb wpa_supplicant wvdial xfsprogs zsh grml-zsh-config-flufflinux fastfetch htop btop traceroute iperf iftop pacman-contrib flufflinux-hooks bind wget cronie unzip bc jq lsof tree ttf-liberation ttf-dejavu ttf-droid ttf-hack thermald base-devel edk2-ovmf git ufw";

const SIGINT: i32 = 2;
const SIGQUIT: i32 = 3;
const SIGTSTP: i32 = 20;
const SIG_IGN: usize = 1;

unsafe extern "C" {
    fn geteuid() -> u32;
    fn signal(signum: i32, handler: usize) -> usize;
}

#[derive(Clone)]
struct Drive {
    device: String,
    model: String,
    size: String,
    size_bytes: u64,
}

fn clear_screen() {
    print!("\x1B[2J\x1B[1;1H");
}

fn print_header() {
    println!("fluffinstall-srv {} - Fluff Linux Server Installer\n", VERSION);
    println!("Welcome to the Fluff Linux Server Installer.");
    println!("This installer will guide you through the process of installing Fluff Linux Server on a device of your choice.\n\n");
}

fn print_stage(title: &str) {
    let stage_indent = "  ";
    let width = 30;
    let padding = width - title.len();
    let left = padding / 2;
    let right = padding - left;

    println!("\n");
    println!("{}┌{}┐", stage_indent, "─".repeat(width + 2));
    println!("{}│ {}{}{} │", stage_indent, " ".repeat(left), title, " ".repeat(right));
    println!("{}└{}┘\n", stage_indent, "─".repeat(width + 2));
}

fn disable_interrupts() {
    unsafe {
        signal(SIGINT, SIG_IGN);
        signal(SIGQUIT, SIG_IGN);
        signal(SIGTSTP, SIG_IGN);
    }
}

fn run_command(command: &str, args: &[&str]) {
    let status = Command::new(command)
    .args(args)
    .status()
    .unwrap_or_else(|_| {
        eprintln!("\n{}Error:{} Failed to run {}.", BOLD_RED, RESET, command);
        std::process::exit(1);
    });

    if !status.success() {
        eprintln!("\n{}Error:{} {} failed.", BOLD_RED, RESET, command);
        std::process::exit(1);
    }
}

fn run_bootloader_command(command: &str, args: &[&str]) {
    let status = Command::new(command)
    .args(args)
    .status()
    .unwrap_or_else(|_| {
        eprintln!(
            "\n{}Error:{} bootloader installation failed! Install cannot continue.",
            BOLD_RED, RESET
        );
        std::process::exit(1);
    });

    if !status.success() {
        eprintln!(
            "\n{}Error:{} bootloader installation failed! Install cannot continue.",
            BOLD_RED, RESET
        );
        std::process::exit(1);
    }
}

fn get_value(line: &str, key: &str) -> String {
    let search = format!("{}=\"", key);

    if let Some(start) = line.find(&search) {
        let value_start = start + search.len();

        if let Some(end) = line[value_start..].find('"') {
            return line[value_start..value_start + end].to_string();
        }
    }

    String::new()
}

fn format_storage_size(bytes: u64) -> String {
    let gib = 1024_u64 * 1024 * 1024;
    let tib = gib * 1024;

    if bytes >= tib {
        let value = bytes as f64 / tib as f64;
        format!("{:.1} TiB", value)
    } else {
        let value = bytes / gib;
        format!("{} GiB", value)
    }
}

fn get_drives() -> Vec<Drive> {
    let output = Command::new("lsblk")
    .args(["-d","-b","-P","--output","NAME,MODEL,SIZE,TYPE","--noheadings"])
    .output()
    .expect("Failed to execute lsblk");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut drives = Vec::new();

    for line in stdout.lines() {
        let device = get_value(line, "NAME");
        let model = get_value(line, "MODEL");
        let size_bytes = get_value(line, "SIZE").parse::<u64>().unwrap_or(0);
        let device_type = get_value(line, "TYPE");

        if device_type == "disk" && !device.is_empty() {
            drives.push(Drive {
                device,
                model: if model.is_empty() { "Unknown".to_string() } else { model },
                        size: format_storage_size(size_bytes),
                        size_bytes,
            });
        }
    }

    drives
}

fn print_drives(drives: &[Drive]) {
    println!("Available drives:\n");

    let index_width = drives.len().to_string().len().max("#".len());
    let device_width = drives.iter().map(|d| d.device.len()).max().unwrap_or("DEVICE".len()).max("DEVICE".len());
    let model_width = drives.iter().map(|d| d.model.len()).max().unwrap_or("MODEL".len()).max("MODEL".len());
    let size_width = drives.iter().map(|d| d.size.len()).max().unwrap_or("SIZE".len()).max("SIZE".len());

    let header = format!(
        "{:<index_width$}   {:<device_width$}  {:<model_width$}  {:<size_width$}",
        "#", "DEVICE", "MODEL", "SIZE"
    );

    println!("{}{}", TABLE_INDENT, header);
    println!("{}{}", TABLE_INDENT, "-".repeat(header.len()));

    for (i, drive) in drives.iter().enumerate() {
        let row = format!(
            "{:<index_width$}   {:<device_width$}  {:<model_width$}  {:<size_width$}",
            i + 1,
            drive.device,
            drive.model,
            drive.size
        );

        println!("{}{}", TABLE_INDENT, row);
        println!("{}{}", TABLE_INDENT, "-".repeat(header.len()));
    }

    println!();
}

fn print_selected_drive(drive: &Drive, index: usize) {
    println!("\nYou have selected:\n");

    let index_width = index.to_string().len().max("#".len());
    let device_width = drive.device.len().max("DEVICE".len());
    let model_width = drive.model.len().max("MODEL".len());
    let size_width = drive.size.len().max("SIZE".len());

    let header = format!(
        "{:<index_width$}   {:<device_width$}  {:<model_width$}  {:<size_width$}",
        "#", "DEVICE", "MODEL", "SIZE"
    );

    println!("{}{}", TABLE_INDENT, header);
    println!("{}{}", TABLE_INDENT, "-".repeat(header.len()));

    let row = format!(
        "{:<index_width$}   {:<device_width$}  {:<model_width$}  {:<size_width$}",
        index,
        drive.device,
        drive.model,
        drive.size
    );

    println!("{}{}", TABLE_INDENT, row);
    println!("{}{}", TABLE_INDENT, "-".repeat(header.len()));
    println!();
}

fn drive_meets_minimum_storage(drive: &Drive) -> bool {
    drive.size_bytes >= MIN_STORAGE_GIB * 1024 * 1024 * 1024
}

fn minimum_storage_error() -> String {
    format!("The selected drive must have at least {} GiB of usable space.", MIN_STORAGE_GIB)
}

fn select_drive(drives: &[Drive]) -> (usize, Drive) {
    let mut error_message = String::new();

    loop {
        clear_screen();
        print_header();

        println!("First, we need to select the target drive.\n\n");
        print_drives(drives);

        if !error_message.is_empty() {
            println!("\n{}Error:{} {}\n", BOLD_RED, RESET, error_message);
        }

        print!("Enter the drive number or DEVICE you want to install Fluff Linux Server on: ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        if input.starts_with("/dev/") {
            error_message = "Enter only the drive number or DEVICE, not the full /dev path.".to_string();
            continue;
        }

        if let Ok(num) = input.parse::<usize>() {
            if num >= 1 && num <= drives.len() {
                let drive = &drives[num - 1];

                if !drive_meets_minimum_storage(drive) {
                    error_message = minimum_storage_error();
                    continue;
                }

                return (num, drive.clone());
            }
        }

        if let Some((index, drive)) = drives.iter().enumerate().find(|(_, drive)| drive.device == input) {
            if !drive_meets_minimum_storage(drive) {
                error_message = minimum_storage_error();
                continue;
            }

            return (index + 1, drive.clone());
        }

        error_message = "Invalid selection. Enter a valid drive number or DEVICE.".to_string();
    }
}

fn confirm_drive(drive: &Drive, index: usize) -> bool {
    print_selected_drive(drive, index);

    println!("{}THIS WILL FORMAT THE SELECTED DRIVE AND REMOVE ALL DATA ON IT.{}", BOLD_RED, RESET);

    loop {
        print!("\nContinue? [y/N]: ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        let input = input.trim();

        if input.is_empty() || input.eq_ignore_ascii_case("n") || input.eq_ignore_ascii_case("no") {
            return false;
        }

        if input.eq_ignore_ascii_case("y") || input.eq_ignore_ascii_case("yes") {
            return true;
        }
    }
}

fn generate_hostname() -> String {
    let mut file = File::open("/dev/urandom").expect("Failed to open /dev/urandom");
    let mut bytes = [0u8; 6];

    file.read_exact(&mut bytes).expect("Failed to read /dev/urandom");

    let first_letter = (b'A' + (bytes[0] % 26)) as char;
    let second_letter = (b'A' + (bytes[1] % 26)) as char;

    let mut digits = String::new();
    for byte in &bytes[2..6] {
        digits.push((b'0' + (byte % 10)) as char);
    }

    format!("FL-{}{}{}", first_letter, second_letter, digits)
}

fn print_hostname_requirements() {
    println!("\n{}{}{}\n", PURPLE, HOSTNAME_REQUIREMENTS, RESET);
}

fn validate_hostname(hostname: &str) -> Result<(), &'static str> {
    if hostname.len() > 255 {
        return Err("The hostname must be 255 characters or less");
    }

    if hostname.chars().any(|c| c.is_whitespace()) {
        return Err("The hostname cannot have spaces");
    }

    if hostname.chars().any(|c| !c.is_ascii_alphanumeric() && c != '-' && c != '.') {
        return Err("The hostname cannot have any special characters (Check requirements and try again)");
    }

    if hostname.starts_with('-') || hostname.starts_with('.') || hostname.ends_with('-') || hostname.ends_with('.') {
        return Err("The hostname cannot start or end with '.' or '-'");
    }

    Ok(())
}

fn print_username_requirements() {
    println!("\n{}{}{}\n", PURPLE, USERNAME_REQUIREMENTS, RESET);
}

fn validate_username(username: &str) -> Result<(), &'static str> {
    if username.len() > 32 {
        return Err("The user name must be 32 characters or less");
    }

    if username.chars().any(|c| c.is_whitespace()) {
        return Err("The user name cannot have spaces");
    }

    if username.starts_with('-') || username.starts_with('_') || username.ends_with('-') || username.ends_with('_') {
        return Err("The user name cannot start or end with '_' or '-'");
    }

    if username.chars().any(|c| !c.is_ascii_alphanumeric() && c != '-' && c != '_') {
        return Err("The user name cannot have any special characters (Check requirements and try again)");
    }

    if username.chars().any(|c| c.is_ascii_uppercase()) {
        return Err("The user name contains uppercase letters, only lowercase letters are allowed");
    }

    if !username.chars().next().unwrap().is_ascii_lowercase() {
        return Err("The user name must start with a lowercase letter");
    }

    if username.chars().all(|c| c.is_ascii_digit()) {
        return Err("The user name cannot be numbers only");
    }

    Ok(())
}

fn confirm_yes_default() -> bool {
    loop {
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        let input = input.trim();

        if input.is_empty() || input.eq_ignore_ascii_case("y") || input.eq_ignore_ascii_case("yes") {
            return true;
        }

        if input.eq_ignore_ascii_case("n") || input.eq_ignore_ascii_case("no") {
            return false;
        }
    }
}

fn hostname_setup() -> String {
    loop {
        println!("\nPlease enter the hostname/system name you'd like to have");
        println!("(for example: server-pc , server1 , flufflinuxsrv)\n");
        println!("If you are not sure, press Enter and the installer will offer a randomized hostname");

        print!("\nHostname: ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        let hostname = input.trim();

        if hostname.is_empty() {
            println!("\nNo hostname was entered. Offering a randomized hostname.\n");

            let generated_hostname = generate_hostname();

            print!("Use generated hostname \"{}\" ? [Y/n]: ", generated_hostname);

            if confirm_yes_default() {
                return generated_hostname;
            }

            println!();
            continue;
        }

        if let Err(error) = validate_hostname(hostname) {
            print_hostname_requirements();
            println!("\n{}Error:{} {}\n", BOLD_RED, RESET, error);
            continue;
        }

        print!("\nSet \"{}\" as the hostname? [Y/n]: ", hostname);

        if confirm_yes_default() {
            return hostname.to_string();
        }

        println!();
    }
}

fn username_setup() -> String {
    loop {
        print!("\nPlease enter the user name you'd like to have: ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        let username = input.trim();

        if username.is_empty() {
            println!("\nNo user name was entered. Offering default user name.\n");
            print!("Use \"user\" as the user name? [Y/n]: ");

            if confirm_yes_default() {
                return "user".to_string();
            }

            println!();
            continue;
        }

        if let Err(error) = validate_username(username) {
            print_username_requirements();
            println!("\n{}Error:{} {}\n", BOLD_RED, RESET, error);
            continue;
        }

        print!("\nSet \"{}\" as the user name? [Y/n]: ", username);

        if confirm_yes_default() {
            return username.to_string();
        }

        println!();
    }
}

fn password_setup(username: &str) {
    println!(
        "\n{}NOTICE!{} For your convenience, the password is visible.\nMake sure only you can see the password.\n",
        YELLOW, RESET
    );

    loop {
        print!("Please enter a password: ");
        io::stdout().flush().unwrap();

        let mut password = String::new();
        io::stdin().read_line(&mut password).unwrap();
        let password = password.trim().to_string();

        if password.is_empty() {
            println!("\n{}Error:{} The password cannot be blank.\n", BOLD_RED, RESET);
            continue;
        }

        print!("\nRe-enter your password to confirm: ");
        io::stdout().flush().unwrap();

        let mut password_recheck = String::new();
        io::stdin().read_line(&mut password_recheck).unwrap();
        let password_recheck = password_recheck.trim();

        if password != password_recheck {
            println!("\n{}Error:{} Passwords do not match. Please try again.\n", BOLD_RED, RESET);
            continue;
        }

        let chpasswd_input = format!("{}:{}\n", username, password);

        let mut child = Command::new("arch-chroot")
        .arg("/mnt")
        .arg("chpasswd")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| {
            eprintln!("\n{}Error:{} Failed to run chpasswd.", BOLD_RED, RESET);
            std::process::exit(1);
        });

        {
            let stdin = child.stdin.as_mut().unwrap_or_else(|| {
                eprintln!("\n{}Error:{} Failed to open chpasswd stdin.", BOLD_RED, RESET);
                std::process::exit(1);
            });

            stdin.write_all(chpasswd_input.as_bytes()).unwrap_or_else(|_| {
                eprintln!("\n{}Error:{} Failed to write password to chpasswd.", BOLD_RED, RESET);
                std::process::exit(1);
            });
        }

        let status = child.wait().unwrap_or_else(|_| {
            eprintln!("\n{}Error:{} Failed to wait for chpasswd.", BOLD_RED, RESET);
            std::process::exit(1);
        });

        if !status.success() {
            eprintln!("\n{}Error:{} Failed to set the user password.", BOLD_RED, RESET);
            std::process::exit(1);
        }

        break;
    }
}

fn disable_swap_on_target(target_disk: &str) {
    let output = Command::new("swapon")
    .args(["--noheadings","--raw","--show=NAME"])
    .output()
    .expect("Failed to run swapon");

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        let swap_device = line.trim();

        if swap_device.is_empty() {
            continue;
        }

        if swap_device.starts_with(target_disk) {
            let status = Command::new("swapoff")
            .arg(swap_device)
            .status()
            .expect("Failed to run swapoff");

            if !status.success() {
                eprintln!("\n{}Error:{} Failed to disable swap on {}.", BOLD_RED, RESET, swap_device);
                std::process::exit(1);
            }
        }
    }
}

fn force_unmount_target(target_disk: &str) {
    let output = Command::new("lsblk")
    .args(["-nr", "-o", "MOUNTPOINT", target_disk])
    .output()
    .expect("Failed to run lsblk");

    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut mountpoints: Vec<String> = stdout
    .lines()
    .map(|line| line.trim().to_string())
    .filter(|line| !line.is_empty())
    .collect();

    mountpoints.sort_by(|a, b| b.len().cmp(&a.len()));

    for mountpoint in mountpoints {
        let _ = Command::new("fuser")
        .args(["-km", &mountpoint])
        .status();

        let status = Command::new("umount")
        .args(["-lf", &mountpoint])
        .status()
        .expect("Failed to run umount");

        if !status.success() {
            eprintln!("\n{}Error:{} Failed to unmount {}.", BOLD_RED, RESET, mountpoint);
            std::process::exit(1);
        }
    }
}

fn main() {
    if unsafe { geteuid() } != 0 {
        eprintln!(
            "\n{}Error:{} {}fluffinstall must be run as root.{}",
            BOLD_RED, RESET, BOLD_RED, RESET
        );
        std::process::exit(1);
    }

    clear_screen();
    print_header();

    loop {
        print!("Continue? [Y/n]: ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        let input = input.trim();

        if input.is_empty() || input.eq_ignore_ascii_case("y") || input.eq_ignore_ascii_case("yes") {
            break;
        }

        if input.eq_ignore_ascii_case("n") || input.eq_ignore_ascii_case("no") {
            std::process::exit(0);
        }
    }

    let drives = get_drives();

    if drives.is_empty() {
        eprintln!("\n{}Error:{} No installable drives were found.", BOLD_RED, RESET);
        std::process::exit(1);
    }

    let (selected_index, selected_drive) = select_drive(&drives);

    if !confirm_drive(&selected_drive, selected_index) {
        std::process::exit(0);
    }

    disable_interrupts();

    let target_disk = format!("/dev/{}", selected_drive.device);

    println!("\nAttempting to format {}...", target_disk);

    disable_swap_on_target(&target_disk);
    force_unmount_target(&target_disk);

    run_command("wipefs", &["--all", &target_disk]);

    run_command("parted", &["--script", &target_disk, "mklabel", "gpt"]);
    run_command("parted", &["--script", &target_disk, "mkpart", "primary", "fat32", "1MiB", "1GiB"]);
    run_command("parted", &["--script", &target_disk, "set", "1", "esp", "on"]);
    run_command("parted", &["--script", &target_disk, "name", "1", "EFI"]);
    run_command("parted", &["--script", &target_disk, "mkpart", "primary", "linux-swap", "1GiB", "5GiB"]);
    run_command("parted", &["--script", &target_disk, "name", "2", "SWAP"]);
    run_command("parted", &["--script", &target_disk, "mkpart", "primary", "ext4", "5GiB", "100%"]);
    run_command("parted", &["--script", &target_disk, "name", "3", "FLS"]);

    let part_suffix = if target_disk.contains("nvme") || target_disk.contains("mmcblk") {
        "p"
    } else {
        ""
    };

    let boot_part = format!("{}{}1", target_disk, part_suffix);
    let swap_part = format!("{}{}2", target_disk, part_suffix);
    let root_part = format!("{}{}3", target_disk, part_suffix);

    run_command("wipefs", &["-a", &boot_part]);
    run_command("wipefs", &["-a", &swap_part]);
    run_command("wipefs", &["-a", &root_part]);

    run_command("mkfs.fat", &["-F32", "-n", "EFI", &boot_part]);
    run_command("mkswap", &["-L", "SWAP", &swap_part]);
    run_command("mkfs.ext4", &["-F", "-L", "FLS", &root_part]);

    run_command("mount", &["-o", "noatime", &root_part, "/mnt"]);
    run_command("mount", &["--mkdir", &boot_part, "/mnt/boot"]);

    run_command("swapon", &[&swap_part]);

    println!("\nInstalling system...\n");

    let mut pacstrap_args: Vec<&str> = vec![
        "-C",
        "/etc/pacman.d/fluffinstall.conf",
        "-K",
        "/mnt",
    ];

    pacstrap_args.extend(PACKAGE_LIST.split_whitespace());

    run_command("pacstrap", &pacstrap_args);

    // Copy custom files
    run_command("cp", &["/etc/os-release","/mnt/etc/"]);
    run_command("cp", &["/usr/lib/os-release","/mnt/usr/lib/"]);
    run_command("cp", &["/etc/motd-real","/mnt/etc/motd"]);
    run_command("cp", &["/etc/issue","/mnt/etc/"]);
    run_command("cp", &["-r","/etc/skel","/mnt/etc/"]);
    run_command("cp", &["/etc/nanorc","/mnt/etc/"]);
    run_command("cp", &["/etc/pacman.conf","/mnt/etc/"]);
    run_command("cp", &["/etc/pacman.d/mirrorlist","/mnt/etc/pacman.d/mirrorlist"]);
    run_command("cp", &["/etc/locale.conf","/mnt/etc/"]);
    run_command("cp",&["/usr/share/pixmaps/flufflinux-logo.png","/mnt/usr/share/pixmaps/"]);
    run_command("ln", &["-sf","/usr/share/zoneinfo/UTC","/mnt/etc/localtime"]); // temporary until timezone setup is added

    // Generate Fstab
    println!("\nGenerating Fstab...");
    run_command("sh",&["-c","genfstab -U /mnt >> /mnt/etc/fstab"]);

    println!("\nGenerating font cache...\n");
    run_command("sh",&["-c","arch-chroot /mnt fc-cache -fv 2>/dev/null"]);

    println!("\nConfiguring bootloader (GRUB) ...");

    run_bootloader_command(
        "arch-chroot",
        &[
            "/mnt",
            "grub-install",
            "--target=x86_64-efi",
            "--efi-directory=/boot",
            "--removable",
            "--boot-directory=/boot",
        ],
    );

    run_bootloader_command("cp", &["/etc/default/grub","/mnt/etc/default/grub"]);
    run_bootloader_command("cp", &["/etc/grub.d/10_linux","/mnt/etc/grub.d/10_linux"]);

    run_bootloader_command(
        "arch-chroot",
        &["/mnt","grub-mkconfig","-o","/boot/grub/grub.cfg"],
    );

    println!("\n{}GRUB successfully installed{}", BOLD_GREEN, RESET);
    println!("\nConfiguring system... \n\n");

    print_stage("Hostname Configuration");
    let hostname = hostname_setup();
    run_command("sh",&["-c",&format!("echo \"{}\" > /mnt/etc/hostname", hostname)]);

    print_stage("User Setup");
    let username = username_setup();

    run_command("arch-chroot",&["/mnt","usermod","-s","/bin/zsh","root"]);

    run_command(
        "arch-chroot",
        &["/mnt","useradd","-m","-G","uucp,wheel","-s","/bin/zsh",&username],
    );

    run_command(
        "arch-chroot",
        &[
            "/mnt",
            "sed",
            "-i",
            "s/^# %wheel ALL=(ALL:ALL) ALL/%wheel ALL=(ALL:ALL) ALL/",
                "/etc/sudoers",
        ],
    );

    run_command(
        "arch-chroot",
        &[
            "/mnt",
            "sh",
            "-c",
            "echo 'Defaults env_keep += \"VISUAL EDITOR\"' >> /etc/sudoers",
        ],
    );

    print_stage("Password Setup");
    password_setup(&username);

    // Enable system services
    run_command("arch-chroot",&["/mnt","systemctl","enable","NetworkManager"]);
    run_command("arch-chroot",&["/mnt","ln","-sf","/run/NetworkManager/resolv.conf","/etc/resolv.conf"]);
    run_command("arch-chroot",&["/mnt","systemctl","enable","fstrim.timer"]);
    run_command("arch-chroot",&["/mnt","systemctl","enable","thermald.service"]);
    run_command("arch-chroot",&["/mnt","systemctl","enable","sshd"]);
    run_command("arch-chroot",&["/mnt","systemctl","enable","systemd-timesyncd"]);
    run_command("arch-chroot",&["/mnt","systemctl","enable","cronie"]);

    let _ = Command::new("pkill").arg("gpg-agent").status();

    run_command("sync",&[]);
    run_command("umount",&["/mnt/boot"]);
    run_command("umount",&["/mnt"]);

    println!("\n\n{}The installation has finished! :){}", BOLD_GREEN, RESET);
    println!("Fluff Linux Server is now bootable on the target drive.");
    println!("Press Enter to exit the installer...");

    let mut exit_input = String::new();
    io::stdin().read_line(&mut exit_input).unwrap();

    std::process::exit(0);
}
