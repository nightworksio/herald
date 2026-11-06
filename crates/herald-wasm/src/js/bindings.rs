use js_sys::{Array, Promise, Uint8Array};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const HOST: &str = r#"
/** A database a host offers, as herald asks for it. */
export type SqlAt =
  | { kind: "sqlite"; file: string }
  | { kind: "postgres"; url: string }
  | { kind: "d1"; binding: string }
  | { kind: "durable-object"; binding: string; object: string };

/** A key-value store a host offers, as herald asks for it. */
export type KvAt =
  | { kind: "deno-kv"; path?: string }
  | { kind: "fastly-kv"; store: string };

/** One request herald sends. The host follows no redirect, reads at most `most` bytes of the body, and, where `public` is set, refuses a host that resolves to an address off the public internet. */
export interface HeraldRequest {
  method: string;
  url: string;
  headers: [string, string][];
  body: Uint8Array;
  most: number;
  public: boolean;
}

/** What a host answered a request with. */
export interface HeraldResponse {
  status: number;
  headers: [string, string][];
  body: Uint8Array;
}

/** A database: runs statements in one transaction, all or none. */
export interface HeraldSql {
  transaction(
    statements: { sql: string; params: (null | number | bigint | string)[] }[],
  ): Promise<{ rows: unknown[][]; changed: number }[]>;
}

/** A key-value store with versioned reads and an atomic check. */
export interface HeraldKv {
  get(key: string[]): Promise<{ value: Uint8Array; versionstamp: string } | null>;
  setIf(key: string[], value: Uint8Array, expected: string | null): Promise<string | null>;
  deleteIf(key: string[], expected: string): Promise<boolean>;
  list(prefix: string[]): Promise<{ key: string[]; value: Uint8Array }[]>;
}

/** What a JavaScript host gives herald. */
export interface HeraldHost {
  /** The variables the host sets, by name. */
  vars: Record<string, string>;
  /** The secret named `name`, or undefined where it is not set. */
  secret(name: string): string | undefined | Promise<string | undefined>;
  /** The response to one request. */
  fetch(request: HeraldRequest): Promise<HeraldResponse>;
  /** The database at `at`; rejects where the host offers none. */
  sql?(at: SqlAt): Promise<HeraldSql>;
  /** The key-value store at `at`; rejects where the host offers none. */
  kv?(at: KvAt): Promise<HeraldKv>;
}
"#;

#[wasm_bindgen]
extern "C" {
    /// What a JavaScript host gives herald.
    #[wasm_bindgen(typescript_type = "HeraldHost")]
    #[derive(Clone)]
    pub type HostObject;

    #[wasm_bindgen(method, getter)]
    pub(crate) fn vars(this: &HostObject) -> JsValue;

    #[wasm_bindgen(method, catch)]
    pub(crate) fn secret(this: &HostObject, name: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch)]
    pub(crate) fn fetch(this: &HostObject, request: &JsValue) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch)]
    pub(crate) fn sql(this: &HostObject, at: &JsValue) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch)]
    pub(crate) fn kv(this: &HostObject, at: &JsValue) -> Result<Promise, JsValue>;

    #[derive(Clone)]
    pub(crate) type SqlObject;

    #[wasm_bindgen(method, catch)]
    pub(crate) fn transaction(this: &SqlObject, statements: &Array) -> Result<Promise, JsValue>;

    #[derive(Clone)]
    pub(crate) type KvObject;

    #[wasm_bindgen(method, catch)]
    pub(crate) fn get(this: &KvObject, key: &Array) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch, js_name = setIf)]
    pub(crate) fn set_if(
        this: &KvObject,
        key: &Array,
        value: &Uint8Array,
        expected: &JsValue,
    ) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch, js_name = deleteIf)]
    pub(crate) fn delete_if(
        this: &KvObject,
        key: &Array,
        expected: &str,
    ) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch)]
    pub(crate) fn list(this: &KvObject, prefix: &Array) -> Result<Promise, JsValue>;

    pub(crate) type Subtle;

    #[wasm_bindgen(method, catch, js_name = importKey)]
    pub(crate) fn import_key(
        this: &Subtle,
        format: &str,
        data: &JsValue,
        algorithm: &JsValue,
        extractable: bool,
        usages: &Array,
    ) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch, js_name = exportKey)]
    pub(crate) fn export_key(
        this: &Subtle,
        format: &str,
        key: &JsValue,
    ) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch)]
    pub(crate) fn sign(
        this: &Subtle,
        algorithm: &JsValue,
        key: &JsValue,
        data: &Uint8Array,
    ) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch)]
    pub(crate) fn verify(
        this: &Subtle,
        algorithm: &JsValue,
        key: &JsValue,
        signature: &Uint8Array,
        data: &Uint8Array,
    ) -> Result<Promise, JsValue>;
}
