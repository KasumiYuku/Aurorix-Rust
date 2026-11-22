use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use hmac::{Hmac, Mac};
use md5::Md5;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use rand::RngCore;
use sha1::{Digest, Sha1};

const GO_QUERY_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

pub fn bkn(skey: &str) -> String {
    let mut hash: i64 = 5381;
    for byte in skey.bytes() {
        hash = hash.wrapping_add(hash.wrapping_shl(5).wrapping_add(i64::from(byte)));
    }
    (hash & 0x7fff_ffff).to_string()
}

pub fn urlencode_go_compat(value: &str) -> String {
    utf8_percent_encode(value, GO_QUERY_SET).to_string()
}

pub fn sha1_hex(data: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn hmac_sha1_hex(key: &str, data: &str) -> String {
    let Ok(mut mac) = Hmac::<Sha1>::new_from_slice(key.as_bytes()) else {
        return String::new();
    };
    mac.update(data.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub fn md5_hex(bytes: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn md5_hex_upper(bytes: &[u8]) -> String {
    md5_hex(bytes).to_uppercase()
}

pub fn md5_base64(bytes: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(bytes);
    BASE64_STANDARD.encode(hasher.finalize())
}

pub fn base64_encode(bytes: &[u8]) -> String {
    BASE64_STANDARD.encode(bytes)
}

pub fn base64_decode(value: &str) -> Option<Vec<u8>> {
    BASE64_STANDARD.decode(value).ok()
}

pub fn uuid_hex() -> String {
    let mut buf = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut buf);
    buf[6] = (buf[6] & 0x0f) | 0x40;
    buf[8] = (buf[8] & 0x3f) | 0x80;
    hex::encode(buf)
}

pub fn cos_canonical(
    method: &str,
    cos_path: &str,
    headers: &[(String, String)],
    now: i64,
    expire: i64,
) -> (String, String, String) {
    let q_key_time = format!("{now};{expire}");
    let mut sorted = headers.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut joined = String::new();
    for (key, value) in &sorted {
        joined.push_str(&urlencode_go_compat(key));
        joined.push('=');
        joined.push_str(&urlencode_go_compat(value));
        joined.push('&');
    }
    let joined = joined.strip_suffix('&').unwrap_or(&joined);
    let format_string = format!("{method}\n{cos_path}\n\n{joined}\n");
    let string_to_sign = format!("sha1\n{q_key_time}\n{}\n", sha1_hex(&format_string));
    (format_string, string_to_sign, q_key_time)
}

pub fn cos_sign(
    method: &str,
    cos_path: &str,
    headers: &[(String, String)],
    secret_key: &str,
    now: i64,
    expire: i64,
) -> (String, String) {
    let (_, string_to_sign, q_key_time) = cos_canonical(method, cos_path, headers, now, expire);
    let sign_key = hmac_sha1_hex(secret_key, &q_key_time);
    let signature = hmac_sha1_hex(&sign_key, &string_to_sign);
    (signature, q_key_time)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bkn_matches_known_vector() {
        assert_eq!(bkn(""), "5381");
        assert_eq!(bkn("abc"), "193485963");
        assert_eq!(bkn("@abcdefg"), "583737345");
    }

    #[test]
    fn urlencode_handles_go_special_cases() {
        assert_eq!(urlencode_go_compat("a b"), "a%20b");
        assert_eq!(urlencode_go_compat("a*b"), "a%2Ab");
        assert_eq!(urlencode_go_compat("~home"), "~home");
        assert_eq!(urlencode_go_compat("a+b"), "a%2Bb");
        assert_eq!(urlencode_go_compat("a/b?c=d"), "a%2Fb%3Fc%3Dd");
    }

    #[test]
    fn cos_canonical_string_shape_matches_go() {
        let headers = vec![
            (
                "host".to_string(),
                "example.cos.ap-nanjing.myqcloud.com".to_string(),
            ),
            ("content-length".to_string(), "12".to_string()),
        ];
        let (format_string, string_to_sign, q_key_time) =
            cos_canonical("put", "/a/b.png", &headers, 1_700_000_000, 1_700_000_600);
        assert_eq!(q_key_time, "1700000000;1700000600");
        assert_eq!(
            format_string,
            "put\n/a/b.png\n\ncontent-length=12&host=example.cos.ap-nanjing.myqcloud.com\n"
        );
        assert!(string_to_sign.starts_with("sha1\n1700000000;1700000600\n"));
        assert!(string_to_sign.ends_with('\n'));
        assert_eq!(string_to_sign.lines().count(), 3);
    }

    #[test]
    fn cos_sign_is_hmac_chain() {
        let headers = vec![("host".to_string(), "h".to_string())];
        let (signature, q_key_time) = cos_sign("put", "/x", &headers, "secret", 100, 200);
        let (_, string_to_sign, _) = cos_canonical("put", "/x", &headers, 100, 200);
        let sign_key = hmac_sha1_hex("secret", &q_key_time);
        assert_eq!(signature, hmac_sha1_hex(&sign_key, &string_to_sign));
        assert_eq!(signature.len(), 40);
        assert_eq!(q_key_time, "100;200");
    }

    #[test]
    fn uuid_is_32_hex_chars_with_version_bits() {
        let first = uuid_hex();
        let second = uuid_hex();
        assert_eq!(first.len(), 32);
        assert_ne!(first, second);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(&first[12..13], "4");
    }

    #[test]
    fn hashes_match_reference_values() {
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(md5_hex_upper(b"abc"), "900150983CD24FB0D6963F7D28E17F72");
        assert_eq!(sha1_hex("abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(md5_base64(b"abc"), "kAFQmDzST7DWlj99KOF/cg==");
    }
}
