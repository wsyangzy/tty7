//! Read the fork's name/path group keys into the workspace-owned group model.

use serde_json::Value;

use super::{AutoKey, GroupId, PinnedGroup, WorkspaceGroups};

pub(crate) fn migrate_workspace(workspace: &mut Value) -> serde_json::Result<()> {
    let Some(object) = workspace.as_object_mut() else {
        return Ok(());
    };
    let mut groups: WorkspaceGroups = object
        .get("groups")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    let mut changed = false;
    if let Some(tabs) = object.get_mut("tabs").and_then(Value::as_array_mut) {
        for tab in tabs {
            changed |= migrate_tab(tab, &mut groups)?;
        }
    }
    if let Some(closed) = object.get_mut("closed").and_then(Value::as_array_mut) {
        for entry in closed {
            if let Some(tab) = entry.get_mut("tab") {
                changed |= migrate_tab(tab, &mut groups)?;
            }
        }
    }
    if changed {
        object.insert("groups".into(), serde_json::to_value(groups)?);
    }
    Ok(())
}

fn migrate_tab(tab: &mut Value, groups: &mut WorkspaceGroups) -> serde_json::Result<bool> {
    let Some(key) = tab.get("group").and_then(Value::as_str) else {
        return Ok(false);
    };
    if serde_json::from_value::<GroupId>(Value::String(key.into())).is_ok() {
        return Ok(false);
    }
    let key = key.to_owned();
    if let Some(name) = key
        .strip_prefix("custom:")
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        let id = match groups
            .pinned
            .iter()
            .find(|g| g.folder.is_none() && g.given_name() == Some(name))
        {
            Some(group) => group.id,
            None => {
                let group = PinnedGroup::label(name);
                let id = group.id;
                groups.pinned.push(group);
                id
            }
        };
        tab["group"] = serde_json::to_value(id)?;
    } else {
        tab["group"] = Value::Null;
        if !key.is_empty()
            && !key.starts_with("custom:")
            && tab.get("last_auto").is_none_or(Value::is_null)
        {
            tab["last_auto"] = serde_json::to_value(AutoKey::Repo(key.into()))?;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::machine::{Machine, Tab, Workspace};

    #[test]
    fn ssh_folder_folds_survive_late_reconnect_and_unfold_per_host() {
        use super::super::GroupKey;
        let a = GroupKey::Auto(AutoKey::SshDirectory("dev@alpha".into(), "/work".into()));
        let b = GroupKey::Auto(AutoKey::SshDirectory("dev@beta".into(), "/work".into()));
        let mut groups = WorkspaceGroups::default();
        let folded = vec!["/work".into()];
        groups.migrate_legacy_folds(&folded);
        assert!(groups.is_folded(Some(&a)));
        assert!(groups.is_folded(Some(&b)));
        groups.toggle_folded(Some(&a));
        let mut groups: WorkspaceGroups =
            serde_json::from_value(serde_json::to_value(groups).unwrap()).unwrap();
        assert!(!groups.migrate_legacy_folds(&folded));
        assert!(!groups.is_folded(Some(&a)));
        assert!(groups.is_folded(Some(&b)));
        groups.toggle_folded(Some(&a));
        assert!(groups.is_folded(Some(&a)));
    }

    #[test]
    fn directory_fallback_waits_for_a_real_answer_and_keeps_host_boundaries() {
        use super::super::auto_key_with_directory;
        use std::path::Path;
        let cwd = Some(Path::new("/work/plain"));
        assert_eq!(auto_key_with_directory(None, None, cwd, true), None);
        assert_eq!(
            auto_key_with_directory(None, Some(None), cwd, false),
            Some(None)
        );
        assert_eq!(
            auto_key_with_directory(None, Some(None), cwd, true),
            Some(Some(AutoKey::Directory("/work/plain".into())))
        );
        assert_eq!(
            auto_key_with_directory(None, Some(Some("/work/repo".into())), cwd, true),
            Some(Some(AutoKey::Repo("/work/repo".into())))
        );
        assert_eq!(
            auto_key_with_directory(Some("dev@host"), Some(None), cwd, true),
            Some(Some(AutoKey::SshHost("dev@host".into())))
        );
    }

    #[test]
    fn old_memberships_keep_names_auto_hints_and_workspace_boundaries() {
        let tab = |group| serde_json::json!({"root":{"Leaf":{"pane":1}}, "group":group});
        let value = serde_json::json!({"workspaces":[
            {"tabs":[tab("custom: 工作 "), tab("custom:工作"), tab("/w/repo"), tab("")]},
            {"tabs":[tab("custom:工作")]}
        ]});
        let machine: Machine = serde_json::from_value(value).unwrap();
        let (a, b) = (&machine.workspaces[0], &machine.workspaces[1]);
        assert_eq!(a.groups.pinned.len(), 1);
        assert_eq!(a.groups.pinned[0].given_name(), Some("工作"));
        assert_eq!(a.tabs[0].group, Some(a.groups.pinned[0].id));
        assert_eq!(a.tabs[0].group, a.tabs[1].group);
        assert_ne!(a.tabs[0].group, b.tabs[0].group);
        assert_eq!(a.tabs[2].group, None);
        assert_eq!(a.tabs[2].last_auto, Some(AutoKey::Repo("/w/repo".into())));
        assert_eq!(a.tabs[3].group, None);
        let saved = serde_json::to_value(&machine).unwrap();
        let reloaded: Machine = serde_json::from_value(saved).unwrap();
        assert_eq!(machine, reloaded);
    }

    #[test]
    fn uuid_memberships_and_new_group_order_are_unchanged() {
        let group = PinnedGroup::label("new");
        let mut tab = Tab::leaf(1);
        tab.group = Some(group.id);
        let ws = Workspace {
            groups: WorkspaceGroups {
                pinned: vec![group],
                ..Default::default()
            },
            tabs: vec![tab],
            ..Default::default()
        };
        let machine = Machine {
            workspaces: vec![ws],
            ..Default::default()
        };
        let read: Machine =
            serde_json::from_value(serde_json::to_value(&machine).unwrap()).unwrap();
        assert_eq!(read, machine);
    }

    #[test]
    fn closed_tabs_share_their_live_groups_and_path_names_stay_labels() {
        let mut value = serde_json::json!({
            "tabs":[{"group":"custom:/w/repo"}],
            "closed":[{"tab":{"group":"custom:/w/repo"}}, {"tab":{"group":"/w/repo"}}]
        });
        migrate_workspace(&mut value).unwrap();
        let groups: WorkspaceGroups = serde_json::from_value(value["groups"].clone()).unwrap();
        assert_eq!(groups.pinned.len(), 1);
        assert_eq!(groups.pinned[0].given_name(), Some("/w/repo"));
        assert_eq!(groups.pinned[0].folder, None);
        assert_eq!(
            value["tabs"][0]["group"],
            value["closed"][0]["tab"]["group"]
        );
        assert_eq!(value["closed"][1]["tab"]["last_auto"]["repo"], "/w/repo");
        let again = value.clone();
        migrate_workspace(&mut value).unwrap();
        assert_eq!(value, again);
    }

    #[test]
    fn old_folds_are_imported_once_and_never_undo_an_unfold() {
        let group = PinnedGroup::label("工作");
        let id = group.id;
        let mut groups = WorkspaceGroups {
            pinned: vec![group],
            ..Default::default()
        };
        let folded = vec!["custom:工作".into(), "/w/repo".into(), "".into()];
        assert!(groups.migrate_legacy_folds(&folded));
        assert!(groups.get(id).unwrap().collapsed);
        assert!(groups.ungrouped_collapsed);
        assert!(
            groups
                .auto_collapsed
                .contains(&AutoKey::Repo("/w/repo".into()))
        );
        assert!(
            groups
                .auto_collapsed
                .contains(&AutoKey::Directory("/w/repo".into()))
        );
        groups.get_mut(id).unwrap().collapsed = false;
        groups.auto_collapsed.clear();
        groups.ungrouped_collapsed = false;
        let mut read: WorkspaceGroups =
            serde_json::from_value(serde_json::to_value(&groups).unwrap()).unwrap();
        assert!(!read.migrate_legacy_folds(&folded));
        assert_eq!(read, groups);
    }
}
