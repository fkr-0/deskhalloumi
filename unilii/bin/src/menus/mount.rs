use super::common::MenuController;
use super::common::{FilterableMenu, QuickjumpMenu};
use super::types::MenuLifecycleState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountState {
    Unmounted,
    Mounting,
    Mounted,
    Error(String),
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDevice {
    pub name: String,
    pub kind: String,
    pub filesystem: Option<String>,
    pub size: Option<String>,
    pub mountpoint: Option<String>,
    pub read_only: bool,
    pub removable: bool,
    pub label: Option<String>,
    pub model: Option<String>,
    pub state: MountState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshfsProfile {
    pub name: String,
    pub user: String,
    pub host: String,
    pub remote_path: String,
    pub mountpoint: String,
    pub options: Vec<String>,
    pub state: MountState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopMount {
    pub image_path: String,
    pub loop_device: Option<String>,
    pub mountpoint: Option<String>,
    pub read_only: bool,
    pub state: MountState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcvolumeProfile {
    pub name: String,
    pub volume_path: String,
    pub mountpoint: String,
    pub command_template: String,
    pub state: MountState,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MountMenuSnapshot {
    pub local_devices: Vec<LocalDevice>,
    pub sshfs_profiles: Vec<SshfsProfile>,
    pub loop_mounts: Vec<LoopMount>,
    pub vcvolume_profiles: Vec<VcvolumeProfile>,
}

#[derive(Debug, Default)]
pub struct MountMenuController {
    lifecycle: MenuLifecycleState,
    snapshot: MountMenuSnapshot,
}

impl MountMenuController {
    pub fn snapshot(&self) -> &MountMenuSnapshot {
        &self.snapshot
    }
}

impl FilterableMenu for MountMenuSnapshot {
    type ItemId = String;

    fn filter_tokens_for(&self, item_id: &Self::ItemId) -> Vec<String> {
        if let Some(device) = self.local_devices.iter().find(|row| &row.name == item_id) {
            return vec![
                device.name.clone(),
                device.kind.clone(),
                device.filesystem.clone().unwrap_or_default(),
                device.label.clone().unwrap_or_default(),
                device.model.clone().unwrap_or_default(),
                device.mountpoint.clone().unwrap_or_default(),
            ];
        }
        if let Some(profile) = self.sshfs_profiles.iter().find(|row| &row.name == item_id) {
            return vec![
                profile.name.clone(),
                profile.host.clone(),
                profile.remote_path.clone(),
                profile.mountpoint.clone(),
            ];
        }
        if let Some(loop_mount) = self
            .loop_mounts
            .iter()
            .find(|row| &row.image_path == item_id)
        {
            return vec![
                loop_mount.image_path.clone(),
                loop_mount.loop_device.clone().unwrap_or_default(),
                loop_mount.mountpoint.clone().unwrap_or_default(),
            ];
        }
        if let Some(profile) = self
            .vcvolume_profiles
            .iter()
            .find(|row| &row.name == item_id)
        {
            return vec![
                profile.name.clone(),
                profile.volume_path.clone(),
                profile.mountpoint.clone(),
            ];
        }
        Vec::new()
    }
}

impl QuickjumpMenu for MountMenuSnapshot {
    type ItemId = String;

    fn quickjump_targets(&self) -> Vec<Self::ItemId> {
        self.local_devices
            .iter()
            .map(|row| row.name.clone())
            .chain(self.sshfs_profiles.iter().map(|row| row.name.clone()))
            .chain(self.loop_mounts.iter().map(|row| row.image_path.clone()))
            .chain(self.vcvolume_profiles.iter().map(|row| row.name.clone()))
            .collect()
    }
}

impl MenuController for MountMenuController {
    type Snapshot = MountMenuSnapshot;

    fn lifecycle_state(&self) -> &MenuLifecycleState {
        &self.lifecycle
    }

    fn lifecycle_state_mut(&mut self) -> &mut MenuLifecycleState {
        &mut self.lifecycle
    }

    fn apply_snapshot(&mut self, snapshot: Self::Snapshot) {
        self.snapshot = snapshot;
        self.lifecycle = MenuLifecycleState::Ready;
    }
}

pub fn parse_lsblk_pairs(output: &str) -> Vec<LocalDevice> {
    let mut devices = Vec::new();

    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let fields = parse_key_value_pairs(line);
        let Some(name) = fields.get("NAME").cloned() else {
            continue;
        };
        let kind = fields
            .get("TYPE")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        let mountpoint = empty_to_none(fields.get("MOUNTPOINT").map(String::as_str));
        let filesystem = empty_to_none(fields.get("FSTYPE").map(String::as_str));

        let state = if mountpoint.is_some() {
            MountState::Mounted
        } else {
            MountState::Unmounted
        };

        devices.push(LocalDevice {
            name,
            kind,
            filesystem,
            size: empty_to_none(fields.get("SIZE").map(String::as_str)),
            mountpoint,
            read_only: parse_bool_field(fields.get("RO").map(String::as_str)),
            removable: parse_bool_field(fields.get("RM").map(String::as_str)),
            label: empty_to_none(fields.get("LABEL").map(String::as_str)),
            model: empty_to_none(fields.get("MODEL").map(String::as_str)),
            state,
        });
    }

    devices
}

pub fn build_mount_command(device: &str, mountpoint: Option<&str>) -> String {
    match mountpoint {
        Some(path) if !path.trim().is_empty() => {
            format!(
                "udisksctl mount -b {} --no-user-interaction && mountpoint {}",
                device,
                shell_escape(path)
            )
        }
        _ => format!("udisksctl mount -b {} --no-user-interaction", device),
    }
}

pub fn build_unmount_command(device: &str) -> String {
    format!("udisksctl unmount -b {} --no-user-interaction", device)
}

pub fn build_sshfs_mount_command(profile: &SshfsProfile) -> String {
    let opts = if profile.options.is_empty() {
        String::new()
    } else {
        format!("-o {} ", profile.options.join(","))
    };
    format!(
        "sshfs {}{}@{}:{} {}",
        opts,
        profile.user,
        profile.host,
        profile.remote_path,
        shell_escape(&profile.mountpoint),
    )
}

pub fn build_sshfs_unmount_command(profile: &SshfsProfile) -> String {
    format!("fusermount -u {}", shell_escape(&profile.mountpoint))
}

pub fn build_loop_attach_command(image_path: &str, read_only: bool) -> String {
    if read_only {
        format!("losetup -f --show --read-only {}", shell_escape(image_path))
    } else {
        format!("losetup -f --show {}", shell_escape(image_path))
    }
}

pub fn build_loop_detach_command(loop_device: &str) -> String {
    format!("losetup -d {}", loop_device)
}

pub fn build_vcvolume_mount_command(profile: &VcvolumeProfile) -> String {
    profile
        .command_template
        .replace("{volume}", &shell_escape(&profile.volume_path))
        .replace("{mountpoint}", &shell_escape(&profile.mountpoint))
}

pub fn parse_losetup_list_row(line: &str) -> Option<(String, bool, String)> {
    let mut parts = line.split_whitespace();
    let loop_device = parts.next()?.to_string();
    let read_only_raw = parts.next()?;
    let image_path = parts.collect::<Vec<_>>().join(" ");
    if image_path.is_empty() {
        return None;
    }
    let read_only = matches!(read_only_raw, "1" | "ro" | "RO" | "true" | "yes");
    Some((loop_device, read_only, image_path))
}

fn parse_key_value_pairs(line: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for token in line.split_whitespace() {
        let mut parts = token.splitn(2, '=');
        let Some(key) = parts.next() else {
            continue;
        };
        let Some(raw_value) = parts.next() else {
            continue;
        };
        let value = raw_value.trim_matches('"').to_string();
        map.insert(key.to_string(), value);
    }
    map
}

fn parse_bool_field(value: Option<&str>) -> bool {
    matches!(value, Some("1") | Some("yes") | Some("true") | Some("on"))
}

fn empty_to_none(value: Option<&str>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn shell_escape(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lsblk_pairs_into_local_devices() {
        let input = r#"NAME="sda1" TYPE="part" FSTYPE="ext4" SIZE="512G" MOUNTPOINT="/data" RO="0" RM="0" LABEL="DATA" MODEL="Samsung"
NAME="sdb1" TYPE="part" FSTYPE="vfat" SIZE="64G" MOUNTPOINT="" RO="0" RM="1" LABEL="USB" MODEL="""#;

        let devices = parse_lsblk_pairs(input);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].name, "sda1");
        assert_eq!(devices[0].state, MountState::Mounted);
        assert_eq!(devices[1].name, "sdb1");
        assert_eq!(devices[1].state, MountState::Unmounted);
        assert!(devices[1].removable);
    }

    #[test]
    fn builds_sshfs_and_loop_commands() {
        let profile = SshfsProfile {
            name: "lab".to_string(),
            user: "dev".to_string(),
            host: "host.local".to_string(),
            remote_path: "/srv/data".to_string(),
            mountpoint: "/mnt/lab".to_string(),
            options: vec![
                "reconnect".to_string(),
                "ServerAliveInterval=15".to_string(),
            ],
            state: MountState::Unmounted,
        };
        assert!(build_sshfs_mount_command(&profile).contains(
            "sshfs -o reconnect,ServerAliveInterval=15 dev@host.local:/srv/data '/mnt/lab'"
        ));
        assert_eq!(
            build_loop_attach_command("/tmp/image.iso", true),
            "losetup -f --show --read-only '/tmp/image.iso'"
        );
    }

    #[test]
    fn renders_vcvolume_template() {
        let profile = VcvolumeProfile {
            name: "vault".to_string(),
            volume_path: "/secure/vault.hc".to_string(),
            mountpoint: "/mnt/vault".to_string(),
            command_template: "veracrypt --text {volume} {mountpoint}".to_string(),
            state: MountState::Unmounted,
        };
        assert_eq!(
            build_vcvolume_mount_command(&profile),
            "veracrypt --text '/secure/vault.hc' '/mnt/vault'"
        );
    }

    #[test]
    fn parses_losetup_row_with_read_only_flag() {
        let parsed =
            parse_losetup_list_row("/dev/loop0 1 /tmp/image.iso").expect("row should parse");
        assert_eq!(parsed.0, "/dev/loop0");
        assert!(parsed.1);
        assert_eq!(parsed.2, "/tmp/image.iso");
    }
}
