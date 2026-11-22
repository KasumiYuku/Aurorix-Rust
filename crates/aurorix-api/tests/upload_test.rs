mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use aurorix_api::{
    MediaUpload, Scene, Target, DEFAULT_UPLOAD_THRESHOLD, MD5_HEAD_SIZE, MEDIA_IMAGE,
};
use support::{token_ok, MockResponse, MockServer, Recorded, TOKEN_PATH};

const PAYLOAD: &[u8] = b"hello aurorix upload";
const PAYLOAD_MD5: &str = "64ed0b3605f7f445d61ec4f65ba1181d";
const PART_ONE_MD5: &str = "248430b204f4f6b8e2b7efc800735b4b";
const PART_TWO_MD5: &str = "3bb9762a9a0bb710464ee4c79f39ac08";
const PART_THREE_MD5: &str = "ec4d1eb36b22d19728e9d1d23ca84d1c";

struct ChunkedMock {
    base: Mutex<String>,
    finishes: AtomicUsize,
    fail_finish_at: Option<usize>,
}

async fn build_chunked_server(fail_finish_at: Option<usize>) -> (MockServer, Arc<ChunkedMock>) {
    let state = Arc::new(ChunkedMock {
        base: Mutex::new(String::new()),
        finishes: AtomicUsize::new(0),
        fail_finish_at,
    });
    let shared = Arc::clone(&state);
    let server =
        MockServer::start(move |record: &Recorded, _index: usize| responder(&shared, record)).await;
    *state.base.lock().expect("基址要能写") = server.base_url.clone();
    (server, state)
}

fn responder(state: &ChunkedMock, record: &Recorded) -> MockResponse {
    if record.path.ends_with(TOKEN_PATH) {
        return token_ok();
    }
    if record.path.ends_with("/upload_prepare") {
        let base = state.base.lock().unwrap().clone();
        let body = format!(
            r#"{{"upload_id":"u1","block_size":"8","parts":[{{"index":1,"presigned_url":"{base}/presign/1"}},{{"index":2,"presigned_url":"{base}/presign/2"}},{{"index":3,"presigned_url":"{base}/presign/3"}}]}}"#
        );
        return MockResponse::json(body);
    }
    if record.path.starts_with("/presign/") {
        return MockResponse::empty();
    }
    if record.path.ends_with("/upload_part_finish") {
        let seen = state.finishes.fetch_add(1, Ordering::SeqCst);
        if Some(seen) == state.fail_finish_at {
            return MockResponse::platform(40093001, "文件上传失败，请重试");
        }
        return MockResponse::empty();
    }
    if record.path.ends_with("/files") {
        return MockResponse::json(
            r#"{"file_info":"fi-1","file_uuid":"fu-1","ttl":3600,"raw_url":"https://img.example/raw.png"}"#,
        );
    }
    MockResponse::empty()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chunked_upload_runs_the_four_steps_in_order() {
    let (server, state) = build_chunked_server(None).await;
    let api = server.api();
    let target = Target::new(Scene::Group, "g1");
    let upload = MediaUpload::image(PAYLOAD.to_vec(), "pic")
        .with_mime_type("image/png")
        .force_chunked();
    let result = api.upload(&target, &upload).await.expect("分片上传要成功");
    assert_eq!(result.file_info, "fi-1");
    assert_eq!(result.file_uuid, "fu-1");
    assert_eq!(result.ttl, 3600);
    assert_eq!(result.raw_url, "https://img.example/raw.png");
    assert_eq!(
        state.finishes.load(Ordering::SeqCst),
        3,
        "三片各 finish 一次"
    );
    let requests = server.api_requests();
    let paths: Vec<String> = requests.iter().map(|record| record.path.clone()).collect();
    assert_eq!(
        paths,
        vec![
            String::from("/v2/groups/g1/upload_prepare"),
            String::from("/presign/1"),
            String::from("/v2/groups/g1/upload_part_finish"),
            String::from("/presign/2"),
            String::from("/v2/groups/g1/upload_part_finish"),
            String::from("/presign/3"),
            String::from("/v2/groups/g1/upload_part_finish"),
            String::from("/v2/groups/g1/files"),
        ],
        "顺序必须是 prepare → PUT → finish → … → files"
    );
    let methods: Vec<String> = requests
        .iter()
        .map(|record| record.method.clone())
        .collect();
    assert_eq!(methods[1], "PUT", "分片用 PUT 直传");
    assert_eq!(methods[0], "POST");
    let prepare = requests[0].json();
    assert_eq!(prepare["file_type"], MEDIA_IMAGE);
    assert_eq!(prepare["file_size"], PAYLOAD.len());
    assert_eq!(prepare["md5"], PAYLOAD_MD5, "整体 md5 要精确");
    assert_eq!(
        prepare["md5_10m"], PAYLOAD_MD5,
        "短内容的分片 md5 与整体一致"
    );
    assert_eq!(prepare["file_name"], "pic");
    assert_eq!(requests[1].body, b"hello au".to_vec(), "第一片字节要对");
    assert_eq!(requests[1].header("content-type"), "image/png");
    assert_eq!(requests[2].json()["md5"], PART_ONE_MD5);
    assert_eq!(requests[2].json()["block_size"], 8);
    assert_eq!(requests[3].body, b"rorix up".to_vec());
    assert_eq!(requests[4].json()["md5"], PART_TWO_MD5);
    assert_eq!(requests[5].body, b"load".to_vec());
    assert_eq!(requests[6].json()["md5"], PART_THREE_MD5);
    assert_eq!(requests[6].json()["block_size"], 4);
    let files = requests[7].json();
    assert_eq!(files["upload_id"], "u1");
    assert_eq!(files["srv_send_msg"], false);
    assert_eq!(files["file_type"], MEDIA_IMAGE);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn single_part_failure_is_retried_not_fatal() {
    let (server, state) = build_chunked_server(Some(1)).await;
    let api = server.api();
    let target = Target::new(Scene::Group, "g1");
    let upload = MediaUpload::image(PAYLOAD.to_vec(), "pic").force_chunked();
    let result = api
        .upload(&target, &upload)
        .await
        .expect("单片失败要能重试成功");
    assert_eq!(result.file_info, "fi-1");
    assert_eq!(
        state.finishes.load(Ordering::SeqCst),
        4,
        "第二片失败一次共 4 次 finish"
    );
    let presign_two = server
        .api_requests()
        .iter()
        .filter(|record| record.path == "/presign/2")
        .count();
    assert_eq!(presign_two, 2, "失败的分片要整片重传");
    assert_eq!(server.api_count(), 10, "重传一片共 10 次请求");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn md5_10m_is_taken_from_the_head() {
    let mut payload = PAYLOAD.to_vec();
    payload.resize(MD5_HEAD_SIZE + 32, b'z');
    let (server, _state) = build_chunked_server(None).await;
    let api = server.api();
    let upload = MediaUpload::image(payload.clone(), "big.png").force_chunked();
    let _ = api.upload(&Target::new(Scene::Group, "g1"), &upload).await;
    let prepare = server.api_requests()[0].json();
    let head = aurorix_api::md5_hex(&payload[..MD5_HEAD_SIZE]);
    assert_eq!(prepare["md5_10m"], head, "md5_10m 只取前 10002432 字节");
    assert_ne!(
        prepare["md5_10m"],
        aurorix_api::md5_hex(&payload),
        "md5_10m 不能等于整体 md5"
    );
}

#[tokio::test]
async fn small_upload_goes_single_shot_with_base64() {
    let server =
        MockServer::with_script(vec![MockResponse::json(r#"{"file_info":"fi-2","ttl":60}"#)]).await;
    let api = server.api();
    let result = api
        .upload_image(&Target::new(Scene::C2c, "u1"), b"abc".to_vec(), "a.png")
        .await
        .expect("小文件走单次上传");
    assert_eq!(result.file_info, "fi-2");
    assert_eq!(server.api_count(), 1);
    let request = server.last();
    assert_eq!(request.path, "/v2/users/u1/files");
    let body = request.json();
    assert_eq!(body["file_data"], "YWJj", "字节要 base64");
    assert_eq!(body["srv_send_msg"], false);
    assert_eq!(body["file_type"], MEDIA_IMAGE);
    assert_eq!(body["file_name"], "a.png");
    assert_eq!(DEFAULT_UPLOAD_THRESHOLD, 3 << 20);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn threshold_switch_forces_chunked() {
    let (server, _state) = build_chunked_server(None).await;
    let api = server.api();
    api.set_upload_threshold(4);
    let _ = api
        .upload_image(&Target::new(Scene::Group, "g1"), b"hello".to_vec(), "a.png")
        .await;
    assert_eq!(
        server.api_requests()[0].path,
        "/v2/groups/g1/upload_prepare",
        "超过阈值必须走分片"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn official_image_requires_chunked_and_raw_url() {
    let (server, _state) = build_chunked_server(None).await;
    let api = server.api();
    let url = api
        .upload_official_image(
            &Target::new(Scene::C2c, "u1"),
            b"abc".to_vec(),
            "a.png",
            "image/png",
        )
        .await
        .expect("官方图床要能拿到 raw_url");
    assert_eq!(url, "https://img.example/raw.png");
    let requests = server.api_requests();
    assert!(
        requests[0].path.ends_with("/upload_prepare"),
        "官方图床即使很小也必须分片"
    );
    assert_eq!(requests[0].path, "/v2/users/u1/upload_prepare");
    let prepare = requests[0].json();
    assert_eq!(prepare["md5"], "900150983cd24fb0d6963f7d28e17f72");
}

#[tokio::test]
async fn official_image_rejects_guild_scene() {
    let server = MockServer::with_script(Vec::new()).await;
    let err = server
        .api()
        .upload_official_image(
            &Target::new(Scene::Channel, "c1"),
            b"abc".to_vec(),
            "a.png",
            "image/png",
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("不支持"));
    assert_eq!(server.api_count(), 0);
}
