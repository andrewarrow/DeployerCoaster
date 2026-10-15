use super::*;
use serde_json::json;

fn app(id: &str) -> AppleApp {
    serde_json::from_value(
        json!({"id": id, "name": format!("App {id}"), "bundleId": format!("com.example.{id}")}),
    )
    .unwrap()
}

#[test]
fn disk_cache_skips_fetch_until_exactly_twenty_four_hours() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("apps.json");
    let first = load_or_fetch(Some(&path), 100, false, || Ok(vec![app("123")])).unwrap();
    assert_eq!(first.apps[0].id, "123");
    let hit = load_or_fetch(Some(&path), 100 + CACHE_SECONDS - 1, false, || {
        panic!("fresh disk cache must skip network")
    })
    .unwrap();
    assert_eq!(hit.fetched_at, 100);
    let expired = load_or_fetch(Some(&path), 100 + CACHE_SECONDS, false, || {
        Ok(vec![app("456")])
    })
    .unwrap();
    assert_eq!(expired.apps[0].id, "456");
    let refreshed = load_or_fetch(Some(&path), 101 + CACHE_SECONDS, true, || Ok(vec![])).unwrap();
    assert!(refreshed.apps.is_empty());
    assert!(
        load_or_fetch(Some(&path), 102 + CACHE_SECONDS, false, || panic!(
            "empty lists are cached too"
        ))
        .unwrap()
        .apps
        .is_empty()
    );
}

#[test]
fn corrupt_future_and_invalid_caches_are_replaced_and_failures_do_not_poison_disk() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("apps.json");
    for bytes in [
        b"broken json".to_vec(),
        serde_json::to_vec(&AppList {
            fetched_at: 101,
            apps: vec![app("1")],
        })
        .unwrap(),
        serde_json::to_vec(&AppList {
            fetched_at: 100,
            apps: vec![app("../bad")],
        })
        .unwrap(),
    ] {
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            load_or_fetch(Some(&path), 100, false, || Ok(vec![app("2")]))
                .unwrap()
                .apps[0]
                .id,
            "2"
        );
    }
    let before = fs::read(&path).unwrap();
    assert!(load_or_fetch(Some(&path), 101, true, || Err("Offline".into())).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    // Storage failures must not prevent an app from being opened.
    assert_eq!(
        load_or_fetch(Some(directory.path()), 100, true, || Ok(vec![app("3")]))
            .unwrap()
            .apps[0]
            .id,
        "3"
    );
}

#[test]
fn recents_survive_restart_move_to_front_and_remain_unique_and_bounded() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("recents.json");
    let mut recents = load_recents(Some(&path));
    for i in 0..15 {
        remember(&mut recents, app(&i.to_string()), Some(&path)).unwrap();
    }
    let mut renamed = app("9");
    renamed.attributes.name = "New name".into();
    remember(&mut recents, renamed, Some(&path)).unwrap();
    let loaded = load_recents(Some(&path));
    assert_eq!(loaded.len(), MAX_RECENTS);
    assert_eq!(loaded[0].id, "9");
    assert_eq!(loaded[0].title(), "New name");
    assert_eq!(loaded.iter().filter(|app| app.id == "9").count(), 1);
}

#[test]
fn account_paths_are_isolated_and_do_not_contain_credentials() {
    let a = AppleCredentials::test_credentials("one");
    let b = AppleCredentials::test_credentials("two");
    assert_ne!(cache_path(&a), cache_path(&b));
    assert_ne!(
        recents_path(&account_key(&a)),
        recents_path(&account_key(&b))
    );
    let mut rotated = a.clone();
    rotated.key_id = "ROTATED123".into();
    rotated.private_key = "rotated private key".into();
    assert_eq!(cache_path(&a), cache_path(&rotated));
    assert!(
        !cache_path(&a)
            .unwrap()
            .to_string_lossy()
            .contains(&a.private_key)
    );
}
