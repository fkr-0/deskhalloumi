#![allow(dead_code)]
// FIXME(T6): Mount menu model is planned toolbar/menu integration surface pending canonical MenuModel wiring.

use super::common::MenuController;
use super::common::{FilterableMenu, QuickjumpMenu};
use super::presentation::{
    ActionItemOptions, action_item, section_item, shell_escape, status_item,
};
use super::types::MenuLifecycleState;
use crate::enhanced_tray::{TrayMenuAction, TrayMenuItem};

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

pub fn build_menu_items(
    app_id: &str,
    snapshot: Option<&MountMenuSnapshot>,
    loading: bool,
    error: Option<&str>,
    config: &deskhalloumi_core::config::MountMenuConfig,
) -> Vec<TrayMenuItem> {
    let mut items = vec![
        action_item(
            app_id,
            "mount-refresh",
            "Refresh storage",
            TrayMenuAction::SpawnCommand("mount:refresh".to_string()),
            ActionItemOptions {
                subtitle: Some("Rescan local devices and configured profiles".to_string()),
                icon: Some("view-refresh".to_string()),
                shortcut: Some("R".to_string()),
                enabled: true,
            },
        ),
        action_item(
            app_id,
            "mount-disks",
            "Disk utility",
            TrayMenuAction::SpawnCommand(config.disks_command.clone()),
            ActionItemOptions {
                subtitle: Some("Open the configured graphical storage manager".to_string()),
                icon: Some("drive-harddisk".to_string()),
                shortcut: None,
                enabled: !config.disks_command.trim().is_empty(),
            },
        ),
    ];
    if loading {
        items.push(status_item(
            app_id,
            "mount-loading",
            "Loading storage snapshot…",
            Some("Devices and remote profiles are being refreshed".to_string()),
        ));
        return items;
    }
    if let Some(error) = error {
        items.push(status_item(
            app_id,
            "mount-error",
            "Storage refresh failed",
            Some(error.to_string()),
        ));
    }
    let Some(snapshot) = snapshot else {
        items.push(status_item(
            app_id,
            "mount-empty-snapshot",
            "No storage snapshot available",
            Some("Use Refresh storage to retry".to_string()),
        ));
        return items;
    };

    items.push(section_item(
        app_id,
        "mount-local",
        "Local devices",
        Some(snapshot.local_devices.len().min(config.max_local_rows)),
    ));
    if snapshot.local_devices.is_empty() {
        items.push(status_item(
            app_id,
            "mount-local-empty",
            "No mountable local devices found",
            None,
        ));
    }
    for device in snapshot.local_devices.iter().take(config.max_local_rows) {
        let path = format!("/dev/{}", device.name);
        let mounted = device.mountpoint.is_some();
        let title = device
            .label
            .as_deref()
            .filter(|label| !label.trim().is_empty())
            .unwrap_or(&device.name)
            .to_string();
        let mut details = Vec::new();
        details.push(path.clone());
        if config.show_device_details {
            if let Some(filesystem) = &device.filesystem {
                details.push(filesystem.clone());
            }
            if let Some(size) = &device.size {
                details.push(size.clone());
            }
            if device.removable {
                details.push("removable".to_string());
            }
            if device.read_only {
                details.push("read-only".to_string());
            }
            if let Some(model) = &device.model {
                details.push(model.clone());
            }
        }
        if let Some(mountpoint) = &device.mountpoint {
            details.push(format!("mounted at {mountpoint}"));
        } else {
            details.push("not mounted".to_string());
        }
        let command = if mounted {
            build_unmount_command(&path)
        } else {
            build_mount_command(&path, None)
        };
        items.push(action_item(
            app_id,
            format!("mount-local:{}", device.name),
            title,
            TrayMenuAction::SpawnCommand(command),
            ActionItemOptions {
                subtitle: Some(details.join(" · ")),
                icon: Some(
                    if device.removable {
                        "drive-removable-media"
                    } else {
                        "drive-harddisk"
                    }
                    .to_string(),
                ),
                shortcut: Some(if mounted { "Unmount" } else { "Mount" }.to_string()),
                enabled: !matches!(device.state, MountState::Mounting),
            },
        ));
    }

    items.push(section_item(
        app_id,
        "mount-sshfs",
        "SSHFS profiles",
        Some(snapshot.sshfs_profiles.len().min(config.max_sshfs_rows)),
    ));
    if snapshot.sshfs_profiles.is_empty() {
        items.push(status_item(
            app_id,
            "mount-sshfs-empty",
            "No SSHFS profiles configured",
            Some("Add profiles under menus.mount.sshfs_profiles".to_string()),
        ));
    }
    for profile in snapshot.sshfs_profiles.iter().take(config.max_sshfs_rows) {
        let mounted = profile.state == MountState::Mounted;
        let subtitle = if mounted {
            format!("{} · mounted at {}", profile.host, profile.mountpoint)
        } else {
            format!(
                "{}@{}:{} · {}",
                profile.user, profile.host, profile.remote_path, profile.mountpoint
            )
        };
        items.push(action_item(
            app_id,
            format!("mount-sshfs:{}", profile.name),
            profile.name.clone(),
            TrayMenuAction::SpawnCommand(if mounted {
                build_sshfs_unmount_command(profile)
            } else {
                build_sshfs_mount_command(profile)
            }),
            ActionItemOptions {
                subtitle: Some(subtitle),
                icon: Some("folder-remote".to_string()),
                shortcut: Some(if mounted { "Unmount" } else { "Mount" }.to_string()),
                enabled: !matches!(profile.state, MountState::Mounting),
            },
        ));
    }

    if config.show_loop_devices {
        items.push(section_item(
            app_id,
            "mount-loop",
            "Loop devices",
            Some(snapshot.loop_mounts.len().min(config.max_loop_rows)),
        ));
        if snapshot.loop_mounts.is_empty() {
            items.push(status_item(
                app_id,
                "mount-loop-empty",
                "No loop images attached",
                None,
            ));
        }
        for loop_mount in snapshot.loop_mounts.iter().take(config.max_loop_rows) {
            let attached = loop_mount.loop_device.is_some();
            let mut details = vec![
                if loop_mount.read_only {
                    "read-only"
                } else {
                    "read-write"
                }
                .to_string(),
            ];
            if let Some(device) = &loop_mount.loop_device {
                details.push(device.clone());
            }
            if let Some(mountpoint) = &loop_mount.mountpoint {
                details.push(format!("mounted at {mountpoint}"));
            }
            items.push(action_item(
                app_id,
                format!("mount-loop:{}", loop_mount.image_path),
                loop_mount
                    .image_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&loop_mount.image_path)
                    .to_string(),
                TrayMenuAction::SpawnCommand(if let Some(device) = &loop_mount.loop_device {
                    build_loop_detach_command(device)
                } else {
                    build_loop_attach_command(&loop_mount.image_path, loop_mount.read_only)
                }),
                ActionItemOptions {
                    subtitle: Some(format!(
                        "{} · {}",
                        loop_mount.image_path,
                        details.join(" · ")
                    )),
                    icon: Some("media-optical".to_string()),
                    shortcut: Some(if attached { "Detach" } else { "Attach" }.to_string()),
                    enabled: !matches!(loop_mount.state, MountState::Mounting),
                },
            ));
        }
    }

    items.push(section_item(
        app_id,
        "mount-vcvolume",
        "Encrypted volumes",
        Some(
            snapshot
                .vcvolume_profiles
                .len()
                .min(config.max_vcvolume_rows),
        ),
    ));
    if snapshot.vcvolume_profiles.is_empty() {
        items.push(status_item(
            app_id,
            "mount-vcvolume-empty",
            "No encrypted-volume profiles configured",
            None,
        ));
    }
    for profile in snapshot
        .vcvolume_profiles
        .iter()
        .take(config.max_vcvolume_rows)
    {
        let mounted = profile.state == MountState::Mounted;
        items.push(action_item(
            app_id,
            format!("mount-vcvolume:{}", profile.name),
            profile.name.clone(),
            TrayMenuAction::SpawnCommand(if mounted {
                format!("umount {}", shell_escape(&profile.mountpoint))
            } else {
                build_vcvolume_mount_command(profile)
            }),
            ActionItemOptions {
                subtitle: Some(format!("{} · {}", profile.volume_path, profile.mountpoint)),
                icon: Some("drive-harddisk-encrypted".to_string()),
                shortcut: Some(if mounted { "Unmount" } else { "Mount" }.to_string()),
                enabled: !matches!(profile.state, MountState::Mounting),
            },
        ));
    }
    items
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
                shell_escape(device),
                shell_escape(path)
            )
        }
        _ => format!(
            "udisksctl mount -b {} --no-user-interaction",
            shell_escape(device)
        ),
    }
}

pub fn build_unmount_command(device: &str) -> String {
    format!(
        "udisksctl unmount -b {} --no-user-interaction",
        shell_escape(device)
    )
}

pub fn build_sshfs_mount_command(profile: &SshfsProfile) -> String {
    let opts = if profile.options.is_empty() {
        String::new()
    } else {
        format!("-o {} ", shell_escape(&profile.options.join(",")))
    };
    let remote = format!("{}@{}:{}", profile.user, profile.host, profile.remote_path);
    format!(
        "sshfs {}{} {}",
        opts,
        shell_escape(&remote),
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
    format!("losetup -d {}", shell_escape(loop_device))
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
    let chars = line.chars().collect::<Vec<_>>();
    let mut cursor = 0usize;
    while cursor < chars.len() {
        while cursor < chars.len() && chars[cursor].is_whitespace() {
            cursor += 1;
        }
        let key_start = cursor;
        while cursor < chars.len() && chars[cursor] != '=' && !chars[cursor].is_whitespace() {
            cursor += 1;
        }
        if cursor >= chars.len() || chars[cursor] != '=' {
            while cursor < chars.len() && !chars[cursor].is_whitespace() {
                cursor += 1;
            }
            continue;
        }
        let key = chars[key_start..cursor].iter().collect::<String>();
        cursor += 1;
        let mut value = String::new();
        if cursor < chars.len() && chars[cursor] == '"' {
            cursor += 1;
            let mut escaped = false;
            while cursor < chars.len() {
                let ch = chars[cursor];
                cursor += 1;
                if escaped {
                    value.push(ch);
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    break;
                } else {
                    value.push(ch);
                }
            }
        } else {
            while cursor < chars.len() && !chars[cursor].is_whitespace() {
                value.push(chars[cursor]);
                cursor += 1;
            }
        }
        if !key.is_empty() {
            map.insert(key, value);
        }
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
            "sshfs -o 'reconnect,ServerAliveInterval=15' 'dev@host.local:/srv/data' '/mnt/lab'"
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
    #[test]
    fn lsblk_parser_preserves_quoted_spaces_and_escapes() {
        let rows = parse_lsblk_pairs(
            r#"NAME="sda1" TYPE="part" FSTYPE="ext4" SIZE="10G" MOUNTPOINT="/media/My Disk" RO="0" RM="1" LABEL="My Disk" MODEL="USB Drive""#,
        );
        assert_eq!(rows[0].mountpoint.as_deref(), Some("/media/My Disk"));
        assert_eq!(rows[0].label.as_deref(), Some("My Disk"));
        assert_eq!(rows[0].model.as_deref(), Some("USB Drive"));
    }

    #[test]
    fn built_storage_items_keep_sections_and_action_metadata() {
        let snapshot = MountMenuSnapshot {
            local_devices: vec![LocalDevice {
                name: "sdb1".into(),
                kind: "part".into(),
                filesystem: Some("ext4".into()),
                size: Some("64G".into()),
                mountpoint: None,
                read_only: false,
                removable: true,
                label: Some("Backup".into()),
                model: Some("USB".into()),
                state: MountState::Unmounted,
            }],
            ..MountMenuSnapshot::default()
        };
        let items = build_menu_items(
            "mount",
            Some(&snapshot),
            false,
            None,
            &deskhalloumi_core::config::MountMenuConfig::default(),
        );
        let row = items
            .iter()
            .find(|item| item.id == "mount-local:sdb1")
            .unwrap();
        assert_eq!(row.shortcut.as_deref(), Some("Mount"));
        assert!(row.label.contains("Backup\n"));
        assert!(items.iter().any(|item| item.id == "section:mount-sshfs"));
    }

    #[test]
    fn command_builders_quote_device_and_remote_values() {
        assert_eq!(
            build_unmount_command("/dev/disk by-id/x"),
            "udisksctl unmount -b '/dev/disk by-id/x' --no-user-interaction"
        );
        assert_eq!(
            build_loop_detach_command("/dev/loop 0"),
            "losetup -d '/dev/loop 0'"
        );
    }
}
