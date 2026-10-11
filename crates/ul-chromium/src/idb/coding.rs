//! O esquema de chaves do IndexedDB do Chromium (`indexed_db_leveldb_coding`): cada chave do LevelDB é um
//! `KeyPrefix` (banco, repositório de objetos, índice) seguido do tipo e dos campos. Só descreve bytes; quem decide
//! quais chaves um lote leva é a camada de transação.

use ul_leveldb::coding::Input;
/// O `EncodeVarInt` do Chromium é o varint do leveldb.
pub use ul_leveldb::coding::put_varint64 as varint;

/// Os bytes de um inteiro sem sinal na menor largura em bytes (little-endian), como `EncodeInt`.
pub fn encode_int(value: u64) -> Vec<u8> {
    let mut bytes = value.to_le_bytes().to_vec();
    while bytes.len() > 1 && bytes.last() == Some(&0) {
        bytes.pop();
    }
    bytes
}

/// O inverso de [`encode_int`].
pub fn decode_int(bytes: &[u8]) -> u64 {
    bytes.iter().rev().fold(0, |value, byte| (value << 8) | u64::from(*byte))
}

/// Texto em UTF-16 big-endian, sem o tamanho (`EncodeString`).
pub fn string(out: &mut Vec<u8>, units: &[u16]) {
    for unit in units {
        out.extend_from_slice(&unit.to_be_bytes());
    }
}

/// Texto com o tamanho em unidades UTF-16 na frente (`EncodeStringWithLength`).
pub fn string_with_length(out: &mut Vec<u8>, units: &[u16]) {
    varint(out, units.len() as u64);
    string(out, units);
}

/// O inverso de [`string_with_length`]: os bytes UTF-16BE do texto (que se ordenam como as unidades).
pub fn read_string_bytes<'a>(input: &mut Input<'a>) -> Option<&'a [u8]> {
    let length = input.varint64().ok()? as usize;
    input.take(length.checked_mul(2)?).ok()
}

pub fn read_string(input: &mut Input) -> Option<Vec<u16>> {
    Some(read_string_bytes(input)?.chunks_exact(2).map(|pair| u16::from_be_bytes([pair[0], pair[1]])).collect())
}

pub fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// O prefixo de toda chave: o primeiro byte guarda a largura (menos um) de cada id, depois os três ids.
pub fn key_prefix(database_id: u64, object_store_id: u64, index_id: u64) -> Vec<u8> {
    let database = encode_int(database_id);
    let store = encode_int(object_store_id);
    let index = encode_int(index_id);
    let mut out = vec![(((database.len() - 1) << 5) | ((store.len() - 1) << 2) | (index.len() - 1)) as u8];
    out.extend(database);
    out.extend(store);
    out.extend(index);
    out
}

/// Os tipos de chave de IndexedDB (`IndexedDBKey`), na ordem em que o Chromium os numera.
#[derive(Clone, Debug, PartialEq)]
pub enum IdbKey {
    String(Vec<u16>),
    Date(f64),
    Number(f64),
    Array(Vec<IdbKey>),
    Binary(Vec<u8>),
}

impl IdbKey {
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            IdbKey::String(text) => {
                out.push(1);
                string_with_length(out, text);
            }
            IdbKey::Date(value) => {
                out.push(2);
                out.extend_from_slice(&value.to_le_bytes());
            }
            IdbKey::Number(value) => {
                out.push(3);
                out.extend_from_slice(&value.to_le_bytes());
            }
            IdbKey::Array(items) => {
                out.push(4);
                varint(out, items.len() as u64);
                items.iter().for_each(|item| item.encode(out));
            }
            IdbKey::Binary(bytes) => {
                out.push(6);
                varint(out, bytes.len() as u64);
                out.extend_from_slice(bytes);
            }
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode(&mut out);
        out
    }
}

/// O `keyPath` de um repositório ou índice (`EncodeIDBKeyPath`): `0x00 0x00` e o tipo (1 texto, 2 lista).
#[derive(Clone, Debug, PartialEq)]
pub enum KeyPath {
    Null,
    String(Vec<u16>),
    Array(Vec<Vec<u16>>),
}

impl KeyPath {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![0, 0];
        match self {
            KeyPath::Null => out.push(0),
            KeyPath::String(path) => {
                out.push(1);
                string_with_length(&mut out, path);
            }
            KeyPath::Array(paths) => {
                out.push(2);
                varint(&mut out, paths.len() as u64);
                paths.iter().for_each(|path| string_with_length(&mut out, path));
            }
        }
        out
    }
}

/// Os tipos de chave de metadado global (prefixo `0 0 0`).
pub mod global {
    use super::*;

    const fn zero_prefix() -> [u8; 4] {
        [0, 0, 0, 0]
    }

    fn with_type(type_byte: u8) -> Vec<u8> {
        let mut key = zero_prefix().to_vec();
        key.push(type_byte);
        key
    }

    pub fn schema_version() -> Vec<u8> {
        with_type(0)
    }
    pub fn max_database_id() -> Vec<u8> {
        with_type(1)
    }
    pub fn data_version() -> Vec<u8> {
        with_type(2)
    }
    /// A chave que mapeia (origem, nome) ao id do banco: o tipo 201, os dois textos com tamanho.
    pub fn database_name(origin: &[u16], name: &[u16]) -> Vec<u8> {
        let mut key = with_type(201);
        string_with_length(&mut key, origin);
        string_with_length(&mut key, name);
        key
    }
}

/// Os campos de metadado de um banco (prefixo `db 0 0`, o tipo é o último byte).
pub mod database {
    use super::*;

    fn field(database_id: u64, field: u8) -> Vec<u8> {
        let mut key = key_prefix(database_id, 0, 0);
        key.push(field);
        key
    }

    pub fn max_object_store_id(database_id: u64) -> Vec<u8> {
        field(database_id, 3)
    }
    pub fn int_version(database_id: u64) -> Vec<u8> {
        field(database_id, 4)
    }
    pub fn blob_key_generator_current_number(database_id: u64) -> Vec<u8> {
        field(database_id, 5)
    }

    /// O nome do repositório de objetos (tipo 200) aponta para o id dele.
    pub fn object_store_name(database_id: u64, name: &[u16]) -> Vec<u8> {
        let mut key = field(database_id, 200);
        string_with_length(&mut key, name);
        key
    }

    /// Um campo do repositório de objetos: tipo 50, o id e o número do campo.
    pub fn object_store_field(database_id: u64, object_store_id: u64, field_number: u8) -> Vec<u8> {
        let mut key = field(database_id, 50);
        varint(&mut key, object_store_id);
        key.push(field_number);
        key
    }

    /// Um campo de índice: tipo 100, o id do repositório, o do índice e o número do campo.
    pub fn index_field(database_id: u64, object_store_id: u64, index_id: u64, field_number: u8) -> Vec<u8> {
        let mut key = field(database_id, 100);
        varint(&mut key, object_store_id);
        varint(&mut key, index_id);
        key.push(field_number);
        key
    }
}

/// Os números de campo do repositório de objetos (`ObjectStoreMetaDataType`).
pub mod store_field {
    pub const NAME: u8 = 0;
    pub const KEY_PATH: u8 = 1;
    pub const AUTO_INCREMENT: u8 = 2;
    pub const EVICTED: u8 = 3;
    pub const LAST_VERSION: u8 = 4;
    pub const MAX_INDEX_ID: u8 = 5;
    pub const HAS_KEY_PATH: u8 = 6;
    pub const KEY_GENERATOR_CURRENT_NUMBER: u8 = 7;
}

/// Os números de campo de índice (`IndexMetaDataType`).
pub mod index_field {
    pub const NAME: u8 = 0;
    pub const UNIQUE: u8 = 1;
    pub const KEY_PATH: u8 = 2;
    pub const MULTI_ENTRY: u8 = 3;
}

/// O índice interno de cada família de registros de um repositório (`KeyPrefix::index_id`).
pub mod record_index {
    pub const OBJECT_STORE_DATA: u64 = 1;
    pub const EXISTS_ENTRY: u64 = 2;
    pub const BLOB_ENTRY: u64 = 3;
    /// O primeiro id de índice do usuário menos um: o `MAX_INDEX_ID` de um repositório novo.
    pub const MINIMUM_INDEX_ID: u64 = 30;
}

/// `ObjectStoreDataKey`: o registro de um repositório (chave primária codificada).
pub fn object_store_data_key(database_id: u64, object_store_id: u64, primary_key: &IdbKey) -> Vec<u8> {
    let mut key = key_prefix(database_id, object_store_id, record_index::OBJECT_STORE_DATA);
    primary_key.encode(&mut key);
    key
}

/// `ExistsEntryKey`: marca que a chave primária existe.
pub fn exists_entry_key(database_id: u64, object_store_id: u64, primary_key: &IdbKey) -> Vec<u8> {
    let mut key = key_prefix(database_id, object_store_id, record_index::EXISTS_ENTRY);
    primary_key.encode(&mut key);
    key
}

/// `IndexDataKey`: a entrada de um índice do usuário, a chave do índice, o número de sequência e a chave primária.
pub fn index_data_key(database_id: u64, object_store_id: u64, index_id: u64, index_key: &IdbKey, sequence_number: u64, primary_key: &IdbKey) -> Vec<u8> {
    let mut key = key_prefix(database_id, object_store_id, index_id);
    index_key.encode(&mut key);
    varint(&mut key, sequence_number);
    primary_key.encode(&mut key);
    key
}

/// O valor de uma entrada de índice: a versão do registro (a mesma do valor dele) e a chave primária.
pub fn index_data_value(record_version: u64, primary_key: &IdbKey) -> Vec<u8> {
    let mut value = Vec::new();
    varint(&mut value, record_version);
    primary_key.encode(&mut value);
    value
}

/// O valor de um registro: a versão do registro (varint) na frente do valor serializado (`v8::wire_value`).
pub fn object_store_data_value(record_version: u64, wire_value: &[u8]) -> Vec<u8> {
    let mut value = Vec::new();
    varint(&mut value, record_version);
    value.extend_from_slice(wire_value);
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text.bytes().filter(|byte| !byte.is_ascii_whitespace() && *byte != b'\\').collect();
        digits.chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("ascii"), 16).expect("hex")).collect()
    }

    #[test]
    fn prefixes_use_the_smallest_width() {
        assert_eq!(key_prefix(0, 0, 0), hex("00 00 00 00"));
        assert_eq!(key_prefix(1, 0, 0), hex("00 01 00 00"));
        assert_eq!(key_prefix(1, 1, 1), hex("00 01 01 01"));
        assert_eq!(key_prefix(1, 1, 31), hex("00 01 01 1f"));
        assert_eq!(key_prefix(300, 1, 1), hex("20 2c 01 01 01"));
    }

    /// Lotes seq=1, 2 e 4 do oráculo: as chaves globais e o nome `db1` da origem `http_127.0.0.1_8000@1`.
    #[test]
    fn global_keys_match_the_oracle() {
        assert_eq!(global::schema_version(), hex("00 00 00 00 00"));
        assert_eq!(global::max_database_id(), hex("00 00 00 00 01"));
        assert_eq!(global::data_version(), hex("00 00 00 00 02"));
        let key = global::database_name(&units("http_127.0.0.1_8000@1"), &units("db1"));
        let expected = hex(
            "00 00 00 00 c9 15 00 68 00 74 00 74 00 70 00 5f 00 31 00 32 00 37 00 2e 00 30 00 2e 00 30 00 2e 00 31 00 5f
             00 38 00 30 00 30 00 30 00 40 00 31 03 00 64 00 62 00 31",
        );
        assert_eq!(key, expected);
    }

    /// Lotes seq=4, 11 e 31 do oráculo: banco 1, repositório `items` (keyPath `id`) e índice `by_name` (id 31).
    #[test]
    fn metadata_keys_match_the_oracle() {
        assert_eq!(database::int_version(1), hex("00 01 00 00 04"));
        assert_eq!(database::blob_key_generator_current_number(1), hex("00 01 00 00 05"));
        assert_eq!(database::max_object_store_id(1), hex("00 01 00 00 03"));
        assert_eq!(database::object_store_field(1, 1, store_field::NAME), hex("00 01 00 00 32 01 00"));
        assert_eq!(database::object_store_field(1, 1, store_field::KEY_GENERATOR_CURRENT_NUMBER), hex("00 01 00 00 32 01 07"));
        assert_eq!(database::object_store_name(1, &units("items")), hex("00 01 00 00 c8 05 00 69 00 74 00 65 00 6d 00 73"));
        assert_eq!(database::index_field(1, 1, 31, index_field::NAME), hex("00 01 00 00 64 01 1f 00"));
        assert_eq!(database::index_field(1, 1, 31, index_field::KEY_PATH), hex("00 01 00 00 64 01 1f 02"));
        assert_eq!(KeyPath::String(units("id")).encode(), hex("00 00 01 02 00 69 00 64"));
        assert_eq!(KeyPath::String(units("name")).encode(), hex("00 00 01 04 00 6e 00 61 00 6d 00 65"));
    }

    /// Lotes seq=41 e seq=45 do oráculo: chave numérica 1.0 no repositório 1, e a entrada do índice 31 para `a`.
    #[test]
    fn record_keys_match_the_oracle() {
        let one = IdbKey::Number(1.0);
        assert_eq!(object_store_data_key(1, 1, &one), hex("00 01 01 01 03 00 00 00 00 00 00 f0 3f"));
        assert_eq!(exists_entry_key(1, 1, &one), hex("00 01 01 02 03 00 00 00 00 00 00 f0 3f"));
        assert_eq!(
            index_data_key(1, 1, 31, &IdbKey::String(units("a")), 0, &one),
            hex("00 01 01 1f 01 01 00 61 00 03 00 00 00 00 00 00 f0 3f")
        );
        assert_eq!(index_data_value(2, &one), hex("02 03 00 00 00 00 00 00 f0 3f"));
        // Lote seq=212 de idb-later: o registro de versão 4 (chave 3.0) leva a versão no valor do índice.
        assert_eq!(index_data_value(4, &IdbKey::Number(3.0)), hex("04 03 00 00 00 00 00 00 08 40"));
        assert_eq!(object_store_data_value(2, &hex("ff 15")), hex("02 ff 15"));
    }
}
