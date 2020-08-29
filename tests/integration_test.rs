// Copyright (C) 2019, 2020 O.S. Systems Sofware LTDA
//
// SPDX-License-Identifier: MIT OR Apache-2.0

use compress_tools::*;

#[test]
fn get_compressed_file_content() {
    let mut source = std::fs::File::open("tests/fixtures/file.txt.gz").unwrap();
    let mut target = Vec::default();

    let written = uncompress_data(&mut source, &mut target).expect("Failed to uncompress the file");
    assert_eq!(
        String::from_utf8_lossy(&target),
        "some_file_content\n",
        "Uncompressed file did not match",
    );
    assert_eq!(written, 18, "Uncompressed bytes count did not match");
}

#[test]
fn get_a_file_from_tar() {
    let mut source = std::fs::File::open("tests/fixtures/tree.tar").unwrap();
    let mut target = Vec::default();

    let written = uncompress_archive_file(&mut source, &mut target, &"tree/branch2/leaf")
        .expect("Failed to get the file");
    assert_eq!(
        String::from_utf8_lossy(&target),
        "Goodbye World\n",
        "Uncompressed file did not match",
    );
    assert_eq!(written, 14, "Uncompressed bytes count did not match");
}

#[test]
fn successfully_list_archive_files() {
    let source = std::fs::File::open("tests/fixtures/tree.tar").unwrap();

    assert_eq!(
        list_archive_files(source).unwrap(),
        vec![
            "tree/".to_string(),
            "tree/branch1/".to_string(),
            "tree/branch1/leaf".to_string(),
            "tree/branch2/".to_string(),
            "tree/branch2/leaf".to_string(),
        ],
        "file list inside the archive did not match"
    );
}

#[tokio::test]
#[cfg(feature = "tokio_support")]
async fn async_successfully_list_archive_files() {
    let source = tokio::fs::File::open("tests/fixtures/tree.tar")
        .await
        .unwrap();

    assert_eq!(
        tokio_support::list_archive_files(source).await.unwrap(),
        vec![
            "tree/".to_string(),
            "tree/branch1/".to_string(),
            "tree/branch1/leaf".to_string(),
            "tree/branch2/".to_string(),
            "tree/branch2/leaf".to_string(),
        ],
        "file list inside the archive did not match"
    );
}

#[test]
#[ignore]
#[cfg(unix)]
fn uncompress_to_dir_preserve_owner() {
    use std::os::unix::fs::MetadataExt;

    let dir = tempfile::TempDir::new().expect("Failed to create the tmp directory");
    let mut source = std::fs::File::open("tests/fixtures/tree.tar").unwrap();

    uncompress_archive(&mut source, dir.path(), Ownership::Preserve)
        .expect("Failed to uncompress the file");

    assert_eq!(
        dir.path().join("tree/branch1/leaf").exists(),
        true,
        "the path doesn't exist"
    );
    assert_eq!(
        dir.path().join("tree/branch2/leaf").exists(),
        true,
        "the path doesn't exist"
    );
    assert_eq!(
        dir.path()
            .join("tree/branch1/leaf")
            .metadata()
            .unwrap()
            .mode()
            % 0o1000,
        0o664,
        "the permissions did not match"
    );
    assert_eq!(
        dir.path()
            .join("tree/branch2/leaf")
            .metadata()
            .unwrap()
            .mode()
            % 0o1000,
        0o664,
        "the permissions did not match"
    );

    let contents = std::fs::read_to_string(dir.path().join("tree/branch2/leaf")).unwrap();
    assert_eq!(
        contents, "Goodbye World\n",
        "Uncompressed file did not match"
    );
}

#[test]
#[ignore]
fn uncompress_same_file_preserve_owner() {
    uncompress_archive(
        &mut std::fs::File::open("tests/fixtures/tree.tar").unwrap(),
        tempfile::TempDir::new()
            .expect("Failed to create the tmp directory")
            .path(),
        Ownership::Preserve,
    )
    .expect("Failed to uncompress the file");
    uncompress_archive(
        &mut std::fs::File::open("tests/fixtures/tree.tar").unwrap(),
        tempfile::TempDir::new()
            .expect("Failed to create the tmp directory")
            .path(),
        Ownership::Preserve,
    )
    .expect("Failed to uncompress the file");
}

#[test]
#[cfg(unix)]
fn uncompress_to_dir_not_preserve_owner() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::TempDir::new().expect("Failed to create the tmp directory");
    let mut source = std::fs::File::open("tests/fixtures/tree.tar").unwrap();

    uncompress_archive(&mut source, dir.path(), Ownership::Ignore)
        .expect("Failed to uncompress the file");

    assert_eq!(
        dir.path().join("tree/branch1/leaf").exists(),
        true,
        "the path doesn't exist"
    );
    assert_eq!(
        dir.path().join("tree/branch2/leaf").exists(),
        true,
        "the path doesn't exist"
    );
    assert_eq!(
        dir.path()
            .join("tree/branch1/leaf")
            .metadata()
            .unwrap()
            .permissions()
            .mode()
            % 0o1000,
        0o664,
        "the permissions did not match"
    );
    assert_eq!(
        dir.path()
            .join("tree/branch2/leaf")
            .metadata()
            .unwrap()
            .permissions()
            .mode()
            % 0o1000,
        0o664,
        "the permissions did not match"
    );

    let contents = std::fs::read_to_string(dir.path().join("tree/branch2/leaf")).unwrap();
    assert_eq!(
        contents, "Goodbye World\n",
        "Uncompressed file did not match"
    );
}

#[test]
fn uncompress_same_file_not_preserve_owner() {
    uncompress_archive(
        &mut std::fs::File::open("tests/fixtures/tree.tar").unwrap(),
        tempfile::TempDir::new()
            .expect("Failed to create the tmp directory")
            .path(),
        Ownership::Ignore,
    )
    .expect("Failed to uncompress the file");
    uncompress_archive(
        &mut std::fs::File::open("tests/fixtures/tree.tar").unwrap(),
        tempfile::TempDir::new()
            .expect("Failed to create the tmp directory")
            .path(),
        Ownership::Ignore,
    )
    .expect("Failed to uncompress the file");
}

#[test]
fn uncompress_truncated_archive() {
    assert!(matches!(
        uncompress_data(
            std::fs::File::open("tests/fixtures/truncated.log.gz").unwrap(),
            Vec::new()
        ),
        Err(Error::Unknown)
    ));
}
