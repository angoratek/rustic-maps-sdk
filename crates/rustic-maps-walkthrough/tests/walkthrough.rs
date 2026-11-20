//! Contract validation, build and manifest tests.

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;

use serde_json::{Value, json};

use rustic_maps_walkthrough::{
    BuildOptions, BuiltManifest, DEFAULT_CONTRACT_PATH, DEFAULT_FLOWS_PATH, MANIFEST_FILE_NAME,
    ManifestSource, ReferenceFlows, ValidateOptions, ValidationReport, WalkthroughContract,
    WalkthroughError, bind, build_site, byte_length, current_source_commit, serve_on, sha256_hex,
    validate_contract, validate_manifest, validate_walkthrough,
};

static TEMPORARY_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn new(label: &str) -> Self {
        let unique = TEMPORARY_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rustic-maps-walkthrough-{}-{unique}-{label}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temporary directory should be creatable");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate lives two levels below the repository root")
        .to_path_buf()
}

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    bytes.extend_from_slice(&13_u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

fn write_png(path: &Path, width: u32, height: u32) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("asset directory should be creatable");
    }
    fs::write(path, png_bytes(width, height)).expect("image should be writable");
}

fn flows_document() -> Value {
    json!({
        "schema_version": 1,
        "flows": [
            {
                "flow_id": "plan_and_navigate",
                "steps": [
                    { "step_id": "open" },
                    { "step_id": "drive" }
                ]
            }
        ]
    })
}

/// A valid three-platform contract with two screens per platform and one
/// generated image per platform.
fn base_contract(repository: &Path) -> Value {
    let aspect_ratio = 1100.0_f64 / 540.0;
    for platform in ["android", "ios", "react_native"] {
        write_png(
            &repository.join(format!("assets/{platform}.png")),
            540,
            1100,
        );
    }
    json!({
        "schema_version": 1,
        "artifact": {
            "name": "test-walkthrough",
            "purpose": "A three-platform contract used by tests."
        },
        "platforms": {
            "android": {
                "display_name": "Android",
                "aspect_ratio": aspect_ratio,
                "screens": [
                    {
                        "id": "first",
                        "step_id": "open",
                        "image": "assets/android.png",
                        "caption": "First screen.",
                        "hotspots": []
                    },
                    {
                        "id": "second",
                        "step_id": "drive",
                        "image": "assets/android.png",
                        "caption": "Second screen.",
                        "hotspots": []
                    }
                ]
            },
            "ios": {
                "display_name": "iOS",
                "aspect_ratio": aspect_ratio,
                "screens": [
                    {
                        "id": "first",
                        "step_id": "open",
                        "image": "assets/ios.png",
                        "caption": "First screen.",
                        "hotspots": []
                    },
                    {
                        "id": "second",
                        "step_id": "drive",
                        "image": "assets/ios.png",
                        "caption": "Second screen.",
                        "hotspots": []
                    }
                ]
            },
            "react_native": {
                "display_name": "React Native",
                "aspect_ratio": aspect_ratio,
                "screens": [
                    {
                        "id": "first",
                        "step_id": "open",
                        "image": "assets/react_native.png",
                        "caption": "First screen.",
                        "hotspots": []
                    },
                    {
                        "id": "second",
                        "step_id": "drive",
                        "image": "assets/react_native.png",
                        "caption": "Second screen.",
                        "hotspots": []
                    }
                ]
            }
        }
    })
}

fn validate_documents(
    contract: &Value,
    repository: &Path,
) -> Result<ValidationReport, WalkthroughError> {
    let contract: WalkthroughContract =
        serde_json::from_value(contract.clone()).expect("contract should deserialize");
    let flows: ReferenceFlows =
        serde_json::from_value(flows_document()).expect("flows should deserialize");
    validate_contract(&contract, &flows, repository)
}

#[test]
fn validates_the_seeded_contract_with_empty_hotspots() {
    let repository = repository_root();
    let report = validate_walkthrough(&ValidateOptions {
        contract_path: repository.join(DEFAULT_CONTRACT_PATH),
        flows_path: repository.join(DEFAULT_FLOWS_PATH),
        repository_root: repository,
    })
    .expect("the committed contract should validate");
    assert_eq!(report.platform_count, 3);
    assert_eq!(report.screen_count, 18);
    assert_eq!(report.hotspot_count, 0);
    assert_eq!(report.asset_count, 18);
}

#[test]
fn rejects_an_unknown_platform() {
    let temporary = TemporaryDirectory::new("unknown-platform");
    let mut contract = base_contract(temporary.path());
    contract["platforms"]["windows"] = contract["platforms"]["android"].clone();
    let result = validate_documents(&contract, temporary.path());
    assert!(
        matches!(
            &result,
            Err(WalkthroughError::UnknownPlatform { platform, .. }) if platform == "windows"
        ),
        "expected an unknown-platform failure, got {result:?}"
    );
}

#[test]
fn rejects_a_step_that_the_reference_flows_do_not_define() {
    let temporary = TemporaryDirectory::new("unknown-step");
    let mut contract = base_contract(temporary.path());
    contract["platforms"]["android"]["screens"][0]["step_id"] = json!("missing_step");
    let result = validate_documents(&contract, temporary.path());
    assert!(
        matches!(
            &result,
            Err(WalkthroughError::UnknownStep { step_id, .. }) if step_id == "missing_step"
        ),
        "expected an unknown-step failure, got {result:?}"
    );
}

#[test]
fn rejects_a_hotspot_that_names_an_unknown_screen() {
    let temporary = TemporaryDirectory::new("unknown-screen");
    let mut contract = base_contract(temporary.path());
    contract["platforms"]["android"]["screens"][0]["hotspots"] = json!([
        {
            "id": "tap-first",
            "label": "Open the next screen",
            "gesture": "tap",
            "rect": { "top": 10, "left": 10, "width": 20, "height": 20 },
            "next_screen": "missing-screen"
        }
    ]);
    let result = validate_documents(&contract, temporary.path());
    assert!(
        matches!(
            &result,
            Err(WalkthroughError::UnknownScreen { next_screen, .. })
                if next_screen == "missing-screen"
        ),
        "expected an unknown-screen failure, got {result:?}"
    );
}

#[test]
fn accepts_a_hotspot_that_names_a_known_screen() {
    let temporary = TemporaryDirectory::new("known-screen");
    let mut contract = base_contract(temporary.path());
    contract["platforms"]["android"]["screens"][0]["hotspots"] = json!([
        {
            "id": "tap-first",
            "label": "Open the next screen",
            "gesture": "tap",
            "rect": { "top": 10, "left": 10, "width": 20, "height": 20 },
            "next_screen": "second"
        }
    ]);
    let report = validate_documents(&contract, temporary.path())
        .expect("a hotspot that names a known screen should validate");
    assert_eq!(report.hotspot_count, 1);
}

#[test]
fn rejects_a_missing_asset() {
    let temporary = TemporaryDirectory::new("missing-asset");
    let mut contract = base_contract(temporary.path());
    contract["platforms"]["android"]["screens"][0]["image"] = json!("assets/absent.png");
    let result = validate_documents(&contract, temporary.path());
    assert!(
        matches!(&result, Err(WalkthroughError::MissingAsset { path }) if path == "assets/absent.png"),
        "expected a missing-asset failure, got {result:?}"
    );
}

#[test]
fn rejects_a_rect_outside_the_frame() {
    let temporary = TemporaryDirectory::new("rect-bounds");
    let mut contract = base_contract(temporary.path());
    contract["platforms"]["android"]["screens"][0]["hotspots"] = json!([
        {
            "id": "tap-first",
            "label": "Open the next screen",
            "gesture": "tap",
            "rect": { "top": 10, "left": 120, "width": 20, "height": 20 },
            "next_screen": "second"
        }
    ]);
    let result = validate_documents(&contract, temporary.path());
    assert!(
        matches!(
            &result,
            Err(WalkthroughError::RectOutOfBounds { hotspot, .. }) if hotspot == "tap-first"
        ),
        "expected a rectangle-bounds failure, got {result:?}"
    );
}

#[test]
fn rejects_an_image_whose_ratio_contradicts_the_contract() {
    let temporary = TemporaryDirectory::new("aspect-ratio");
    let mut contract = base_contract(temporary.path());
    contract["platforms"]["android"]["aspect_ratio"] = json!(1.0);
    let result = validate_documents(&contract, temporary.path());
    assert!(
        matches!(
            &result,
            Err(WalkthroughError::AspectRatioMismatch { platform, .. }) if platform == "android"
        ),
        "expected an aspect-ratio failure, got {result:?}"
    );
}

#[test]
fn rejects_a_manifest_commit_that_the_repository_cannot_resolve() {
    let repository = repository_root();
    let Some(_) = current_source_commit(&repository).expect("git should be inspectable") else {
        eprintln!("skipping: the repository has no HEAD commit to resolve");
        return;
    };
    let manifest = BuiltManifest {
        schema_version: 1,
        artifact: "test-walkthrough".to_owned(),
        source_commit: Some("0000000000000000000000000000000000000000".to_owned()),
        contract: ManifestSource {
            path: "platform/reference-walkthrough.json".to_owned(),
            sha256: sha256_hex(b""),
        },
        player: Vec::new(),
        assets: Vec::new(),
    };
    let result = validate_manifest(&manifest, &repository);
    assert!(
        matches!(
            &result,
            Err(WalkthroughError::UnreachableSourceCommit { .. })
        ),
        "expected an unreachable-commit failure, got {result:?}"
    );
}

#[test]
fn skips_the_manifest_commit_check_without_a_git_repository() {
    let temporary = TemporaryDirectory::new("no-git");
    let manifest = BuiltManifest {
        schema_version: 1,
        artifact: "test-walkthrough".to_owned(),
        source_commit: Some("0123456789abcdef".to_owned()),
        contract: ManifestSource {
            path: "contract.json".to_owned(),
            sha256: sha256_hex(b""),
        },
        player: Vec::new(),
        assets: Vec::new(),
    };
    let report =
        validate_manifest(&manifest, temporary.path()).expect("a missing repository should skip");
    assert!(!report.source_commit_checked);
    assert!(
        report.notes.iter().any(|note| note.contains("not checked")),
        "expected a clear skip note, got {:?}",
        report.notes
    );
}

#[test]
fn builds_the_seeded_contract_and_writes_matching_hashes() {
    let repository = repository_root();
    let temporary = TemporaryDirectory::new("build");
    let output_directory = temporary.path().join("site");
    let report = build_site(&BuildOptions {
        contract_path: repository.join(DEFAULT_CONTRACT_PATH),
        flows_path: repository.join(DEFAULT_FLOWS_PATH),
        repository_root: repository.clone(),
        output_directory: output_directory.clone(),
    })
    .expect("the seeded contract should build");

    assert_eq!(report.asset_count, 18);
    assert_eq!(report.file_count, 18 + 3 + 1);

    let index_path = output_directory.join("index.html");
    let index_html = fs::read_to_string(&index_path).expect("the player document should exist");
    assert!(index_html.contains("id=\"walkthrough-data\""));
    assert!(index_html.contains("\"platform_order\":[\"android\",\"ios\",\"react_native\"]"));
    assert!(
        !index_html.contains("authoring.js"),
        "the production player must not reference the authoring overlay"
    );
    assert!(!output_directory.join("authoring.js").exists());
    assert!(output_directory.join("player.css").is_file());
    assert!(output_directory.join("player.js").is_file());

    let manifest = BuiltManifest::from_file(&output_directory.join(MANIFEST_FILE_NAME))
        .expect("the manifest should be readable");
    assert_eq!(manifest.assets.len(), 18);
    assert_eq!(manifest.player.len(), 3);
    assert_eq!(
        manifest.contract.sha256,
        sha256_hex(
            &fs::read(repository.join(DEFAULT_CONTRACT_PATH))
                .expect("the contract should be readable")
        )
    );

    for asset in &manifest.assets {
        let bytes = fs::read(output_directory.join(&asset.output))
            .expect("every manifest asset should exist");
        assert_eq!(
            sha256_hex(&bytes),
            asset.sha256,
            "hash for {}",
            asset.output
        );
        assert_eq!(
            byte_length(&bytes),
            asset.bytes,
            "size for {}",
            asset.output
        );
        assert!(
            output_directory
                .join(&asset.output)
                .starts_with(&output_directory),
            "asset paths stay inside the output directory"
        );
    }
    for file in &manifest.player {
        let bytes = fs::read(output_directory.join(&file.output))
            .expect("every manifest player file should exist");
        assert_eq!(sha256_hex(&bytes), file.sha256, "hash for {}", file.output);
        assert_eq!(byte_length(&bytes), file.bytes, "size for {}", file.output);
    }

    let head = current_source_commit(&repository).expect("git should be inspectable");
    if let Some(head) = head {
        assert_eq!(manifest.source_commit, Some(head));
        let manifest_report = validate_manifest(&manifest, &repository)
            .expect("the recorded commit should be reachable");
        assert!(manifest_report.source_commit_checked);
    }
}

#[test]
fn serves_the_built_player_and_injects_the_authoring_overlay() {
    let repository = repository_root();
    let temporary = TemporaryDirectory::new("serve");
    let output_directory = temporary.path().join("site");
    build_site(&BuildOptions {
        contract_path: repository.join(DEFAULT_CONTRACT_PATH),
        flows_path: repository.join(DEFAULT_FLOWS_PATH),
        repository_root: repository,
        output_directory: output_directory.clone(),
    })
    .expect("the seeded contract should build");

    let manifest = BuiltManifest::from_file(&output_directory.join(MANIFEST_FILE_NAME))
        .expect("the manifest should be readable");
    let first_asset = manifest
        .assets
        .first()
        .expect("the manifest should list assets")
        .output
        .clone();

    let listener = bind(SocketAddr::from(([127, 0, 0, 1], 0))).expect("a port should bind");
    let address = listener.local_addr().expect("the port should be known");
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = Arc::clone(&shutdown);
    let server_directory = output_directory.clone();
    let server = thread::spawn(move || serve_on(listener, server_directory, true, server_shutdown));

    let (status, headers, body) = http_get(address, "/");
    assert!(status.starts_with("HTTP/1.1 200"), "status was {status}");
    assert!(headers.contains("text/html"));
    let index_html = String::from_utf8(body).expect("the player document is UTF-8");
    assert!(index_html.contains("authoring.js"));

    let (status, headers, body) = http_get(address, &format!("/{first_asset}"));
    assert!(status.starts_with("HTTP/1.1 200"), "status was {status}");
    assert!(headers.contains("image/png"));
    let expected = fs::read(output_directory.join(&first_asset)).expect("the asset should exist");
    assert_eq!(body, expected);

    let (status, _, _) = http_get(address, "/../Cargo.toml");
    assert!(status.starts_with("HTTP/1.1 404"), "status was {status}");

    shutdown.store(true, Ordering::Relaxed);
    server
        .join()
        .expect("the server thread should finish")
        .expect("the server should stop cleanly");
}

fn http_get(address: SocketAddr, path: &str) -> (String, String, Vec<u8>) {
    let mut stream = TcpStream::connect(address).expect("the server should accept connections");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .expect("the request should be writable");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .expect("the response should be readable");
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the response should carry headers");
    let header = String::from_utf8_lossy(&response[..header_end]).into_owned();
    let body = response[header_end + 4..].to_vec();
    let status = header.lines().next().unwrap_or_default().to_owned();
    (status, header, body)
}
