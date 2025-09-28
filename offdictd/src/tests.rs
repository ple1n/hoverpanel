use anyhow::Result;
use core::fmt::Debug;
use serde::{Deserialize, Serialize};
pub use serde_yaml::{self};
use std::cmp;
use std::collections::BTreeMap;
use std::error::Error;

pub const FIXTURE_PATH: &'static str = "./offdictd/fixtures/dict.yaml";

use crate::def::SrcDef;
use crate::def_bin::{Def, WrapperDef};
use crate::{def, def_bin, DefItemWrapped};
use bincode::Options;
// use postcard;
use std::fs::File;
use std::ops::Deref;

pub fn load_fixture() -> Result<Vec<SrcDef>> {
    let file = File::open(FIXTURE_PATH).expect("Unable to open file");
    let yaml_defs: Vec<SrcDef> = serde_yaml::from_reader(file)?;

    Ok(yaml_defs)
}

pub fn collect_defs(defs: Vec<Def>) -> BTreeMap<String, DefItemWrapped> {
    let mut map = BTreeMap::new();
    let sd = String::new();
    for de in defs {
        let word = de.word.as_ref().unwrap();
        if !map.contains_key(word) {
            map.insert(word.to_owned(), Default::default());
        }
        let for_word: &mut WrapperDef = map.get_mut(de.word.as_ref().unwrap()).unwrap();
        if for_word.word.is_empty() {
            for_word.word = word.to_owned();
        }
        let dict_name = de.dictName.as_ref().unwrap_or(&sd);
        if !for_word.items.contains_key(dict_name) {
            for_word
                .items
                .insert(dict_name.to_owned(), Default::default());
        }
        let dict_def = for_word.items.get_mut(dict_name).unwrap();
        if dict_def.definitions.is_none() {
            dict_def.definitions = Some(Default::default());
        }
        dict_def.definitions.as_mut().unwrap().push(de);
    }

    map
}

fn test_bincode<T: for<'a> Deserialize<'a> + Serialize + Debug + PartialEq>(value: T) {
    let record_coder = bincode::DefaultOptions::new()
        .with_varint_encoding()
        .with_little_endian();

    let value_bytes = record_coder.serialize(&value).unwrap();

    println!(
        "{:?} {}",
        &value_bytes[..cmp::min(value_bytes.len(), 20)],
        value_bytes.len()
    );

    let value_d: T = record_coder.deserialize(&value_bytes).unwrap();
    assert_eq!(value, value_d);
}

// fn test_postcard<T: for<'a> Deserialize<'a> + Serialize + Debug + PartialEq>(value: T) {
//     let value_bytes: _ = postcard::to_vec::<T, 64>(&value).unwrap(); // buffer full

//     println!(
//         "{:?} {}",
//         &value_bytes[..cmp::min(value_bytes.len(), 20)],
//         value_bytes.len()
//     );

//     let value_d: T = postcard::from_bytes(value_bytes.deref()).unwrap();
//     assert_eq!(value, value_d);
// }

#[test]
#[should_panic] // The original struct is too weird ...
fn bincode_orig_def() {
    let value: SrcDef = SrcDef::default();

    test_bincode(value);
}

#[test]
fn bincode_def() {
    let value: Def = Def::default();
    // fails if using `#[serde(skip_serializing_if = "Option::is_none")]`
    test_bincode(value);
}

// #[test]
// fn postcard_def() {
//     let value: def_bin::Def = def_bin::Def::default();

//     test_postcard(value);
//     test_postcard(def_bin::Def {
//         word: Some("bincode".to_owned()),
//         dictName: Some("bincode".to_owned()),
//         ..Default::default()
//     });
// }

#[test]
fn editable_formats() {
    let d = SrcDef {
        word: Some("bincode".to_owned()),
        dictName: Some("bincode".to_owned()),
        ..Default::default()
    };

    println!("{}", serde_yaml::to_string(&d).unwrap());
    let d_: Def = d.clone().for_machine();
    dbg!(&d_);
    let d1: SrcDef = d_.into();

    assert_eq!(d, d1);
}

#[test]
fn yaml() {
    let value: SrcDef = SrcDef::default();

    let mut value_bytes = Vec::new();
    serde_yaml::to_writer(&mut value_bytes, &value).unwrap();

    let value_d: SrcDef = serde_yaml::from_reader(value_bytes.as_slice()).unwrap();
    assert_eq!(value, value_d);
}

#[test]
fn from_yaml() -> Result<(), Box<dyn Error>> {
    let d = load_fixture()?;

    let vec_d: Vec<Def> = d
        .into_iter()
        .map(|mut x| x.normalize_def().into())
        .collect();
    // loads sources of old format and turns them into new format

    for dn in vec_d {
        let dn_: SrcDef = dn.clone().into(); // human-oriented format
        println!("{}", serde_yaml::to_string(&dn_).unwrap());
        assert_eq!(dn, dn_.for_machine());

        test_bincode(dn) // let's just use bincode, not postcard
    }

    Ok(())
}

// #[test]
// fn db() -> Result<(), Box<dyn Error>> {
//     let mut db = open_db("../test_db");
//     db.reset()?;

//     let mut yaml_defs: Vec<Def> = Vec::new();

//     Def::import_yaml(&mut db, yaml_defs, FIXTURE_PATH, &"dict1".to_owned());

//     let r = search(&db, "privacy");
//     assert_eq!(r[0].0.word, "privacy");
//     assert_eq!(r[0].1, 0);
//     assert!(r.len() > 1);

//     let s = search_single(&mut db, "privacy").unwrap();
//     assert_eq!(r[0].0, s);

//     assert!(search_single(&mut db, "privacyx").is_none());

//     println!("{:#?}", r);

//     export_all_yaml(&db, "./out.yaml");
//     export_to_file(&db, "./out.bin");

//     Ok(())
// }
