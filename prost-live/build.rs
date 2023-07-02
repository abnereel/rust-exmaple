use prost_build::Config;

fn main() {
    // 只有当person.proto或build.rs文件改变时才会重新build
    println!("cargo:rerun-if-changed=person.proto");
    println!("cargo:rerun-if-changed=build.rs");
    Config::new()
        .out_dir("src/pb")
        // 把生成后数据中所有的vec[u8]类型改成Bytes类型，
        // 需要先在Cargo.toml dependencies 依赖中加入bytes crate
        // .bytes(&["."])
        // 把生成后数据中的scores域的HashMap类型替换为BTreeMap类型
        .btree_map(&["scores"])
        // 给所有类型添加属性 #[derive(Serialize, Deserialize)]
        // 下面这里需要使用全称，因为serde这个crate有可能没有引入
        .type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        // 给 data field 添加属性 #[serde(skip_serializing_if = "Vec::is_empty")]
        .field_attribute("data", "#[serde(skip_serializing_if = \"Vec::is_empty\")]")
        .compile_protos(&["person.proto"], &["."])
        .unwrap();
}