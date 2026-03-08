use aes::cipher::{BlockDecryptMut, KeyIvInit};
use thiserror::Error;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("解密失败: {0}")]
    DecryptError(String),
    #[error("无效的密钥长度")]
    InvalidKeyLength,
    #[error("无效的 IV 长度")]
    InvalidIvLength,
}

/// AES-128-CBC 解密
///
/// * `data` - 待解密的数据（密文）
/// * `key` - 16 字节 AES 密钥
/// * `iv` - 16 字节初始化向量
pub fn decrypt_aes128(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if key.len() != 16 {
        return Err(CryptoError::InvalidKeyLength);
    }
    if iv.len() != 16 {
        return Err(CryptoError::InvalidIvLength);
    }

    let mut buf = data.to_vec();

    let decryptor = Aes128CbcDec::new_from_slices(key, iv)
        .map_err(|e| CryptoError::DecryptError(e.to_string()))?;

    let decrypted = decryptor
        .decrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(&mut buf)
        .map_err(|e| CryptoError::DecryptError(e.to_string()))?;

    Ok(decrypted.to_vec())
}
