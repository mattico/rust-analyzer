mod globs;
mod incremental;
mod macros;
mod mod_resolution;
mod diagnostics;
mod primitives;
mod block;

use std::sync::Arc;

use base_db::{fixture::WithFixture, SourceDatabase};
use expect_test::{expect, Expect};
use hir_expand::db::AstDatabase;
use test_utils::mark;

use crate::{db::DefDatabase, nameres::*, test_db::TestDB};

fn compute_crate_def_map(ra_fixture: &str) -> Arc<DefMap> {
    let db = TestDB::with_files(ra_fixture);
    let krate = db.crate_graph().iter().next().unwrap();
    db.crate_def_map(krate)
}

fn compute_block_def_map(ra_fixture: &str) -> Arc<DefMap> {
    let (db, position) = TestDB::with_position(ra_fixture);
    let module = db.module_for_file(position.file_id);
    let ast_map = db.ast_id_map(position.file_id.into());
    let ast = db.parse(position.file_id);
    let block: ast::BlockExpr =
        syntax::algo::find_node_at_offset(&ast.syntax_node(), position.offset).unwrap();
    let block_id = ast_map.ast_id(&block);

    db.block_def_map(module.krate, InFile::new(position.file_id.into(), block_id))
}

fn check(ra_fixture: &str, expect: Expect) {
    let def_map = compute_crate_def_map(ra_fixture);
    let actual = def_map.dump();
    expect.assert_eq(&actual);
}

fn check_at(ra_fixture: &str, expect: Expect) {
    let def_map = compute_block_def_map(ra_fixture);
    let actual = def_map.dump();
    expect.assert_eq(&actual);
}

#[test]
fn crate_def_map_smoke_test() {
    check(
        r#"
//- /lib.rs
mod foo;
struct S;
use crate::foo::bar::E;
use self::E::V;

//- /foo/mod.rs
pub mod bar;
fn f() {}

//- /foo/bar.rs
pub struct Baz;

union U { to_be: bool, not_to_be: u8 }
enum E { V }

extern {
    type Ext;
    static EXT: u8;
    fn ext();
}
"#,
        expect![[r#"
            crate
            E: t
            S: t v
            V: t v
            foo: t

            crate::foo
            bar: t
            f: v

            crate::foo::bar
            Baz: t v
            E: t
            EXT: v
            Ext: t
            U: t
            ext: v
        "#]],
    );
}

#[test]
fn crate_def_map_super_super() {
    check(
        r#"
mod a {
    const A: usize = 0;
    mod b {
        const B: usize = 0;
        mod c {
            use super::super::*;
        }
    }
}
"#,
        expect![[r#"
            crate
            a: t

            crate::a
            A: v
            b: t

            crate::a::b
            B: v
            c: t

            crate::a::b::c
            A: v
            b: t
        "#]],
    );
}

#[test]
fn crate_def_map_fn_mod_same_name() {
    check(
        r#"
mod m {
    pub mod z {}
    pub fn z() {}
}
"#,
        expect![[r#"
            crate
            m: t

            crate::m
            z: t v

            crate::m::z
        "#]],
    );
}

#[test]
fn bogus_paths() {
    mark::check!(bogus_paths);
    check(
        r#"
//- /lib.rs
mod foo;
struct S;
use self;

//- /foo/mod.rs
use super;
use crate;
"#,
        expect![[r#"
            crate
            S: t v
            foo: t

            crate::foo
        "#]],
    );
}

#[test]
fn use_as() {
    check(
        r#"
//- /lib.rs
mod foo;
use crate::foo::Baz as Foo;

//- /foo/mod.rs
pub struct Baz;
"#,
        expect![[r#"
            crate
            Foo: t v
            foo: t

            crate::foo
            Baz: t v
        "#]],
    );
}

#[test]
fn use_trees() {
    check(
        r#"
//- /lib.rs
mod foo;
use crate::foo::bar::{Baz, Quux};

//- /foo/mod.rs
pub mod bar;

//- /foo/bar.rs
pub struct Baz;
pub enum Quux {};
"#,
        expect![[r#"
            crate
            Baz: t v
            Quux: t
            foo: t

            crate::foo
            bar: t

            crate::foo::bar
            Baz: t v
            Quux: t
        "#]],
    );
}

#[test]
fn re_exports() {
    check(
        r#"
//- /lib.rs
mod foo;
use self::foo::Baz;

//- /foo/mod.rs
pub mod bar;
pub use self::bar::Baz;

//- /foo/bar.rs
pub struct Baz;
"#,
        expect![[r#"
            crate
            Baz: t v
            foo: t

            crate::foo
            Baz: t v
            bar: t

            crate::foo::bar
            Baz: t v
        "#]],
    );
}

#[test]
fn std_prelude() {
    mark::check!(std_prelude);
    check(
        r#"
//- /main.rs crate:main deps:test_crate
use Foo::*;

//- /lib.rs crate:test_crate
mod prelude;
#[prelude_import]
use prelude::*;

//- /prelude.rs
pub enum Foo { Bar, Baz };
"#,
        expect![[r#"
            crate
            Bar: t v
            Baz: t v
        "#]],
    );
}

#[test]
fn can_import_enum_variant() {
    mark::check!(can_import_enum_variant);
    check(
        r#"
enum E { V }
use self::E::V;
"#,
        expect![[r#"
            crate
            E: t
            V: t v
        "#]],
    );
}

#[test]
fn edition_2015_imports() {
    check(
        r#"
//- /main.rs crate:main deps:other_crate edition:2015
mod foo;
mod bar;

//- /bar.rs
struct Bar;

//- /foo.rs
use bar::Bar;
use other_crate::FromLib;

//- /lib.rs crate:other_crate edition:2018
struct FromLib;
"#,
        expect![[r#"
            crate
            bar: t
            foo: t

            crate::bar
            Bar: t v

            crate::foo
            Bar: t v
            FromLib: t v
        "#]],
    );
}

#[test]
fn item_map_using_self() {
    check(
        r#"
//- /lib.rs
mod foo;
use crate::foo::bar::Baz::{self};

//- /foo/mod.rs
pub mod bar;

//- /foo/bar.rs
pub struct Baz;
"#,
        expect![[r#"
            crate
            Baz: t v
            foo: t

            crate::foo
            bar: t

            crate::foo::bar
            Baz: t v
        "#]],
    );
}

#[test]
fn item_map_across_crates() {
    check(
        r#"
//- /main.rs crate:main deps:test_crate
use test_crate::Baz;

//- /lib.rs crate:test_crate
pub struct Baz;
"#,
        expect![[r#"
            crate
            Baz: t v
        "#]],
    );
}

#[test]
fn extern_crate_rename() {
    check(
        r#"
//- /main.rs crate:main deps:alloc
extern crate alloc as alloc_crate;
mod alloc;
mod sync;

//- /sync.rs
use alloc_crate::Arc;

//- /lib.rs crate:alloc
struct Arc;
"#,
        expect![[r#"
            crate
            alloc_crate: t
            sync: t

            crate::sync
            Arc: t v
        "#]],
    );
}

#[test]
fn extern_crate_rename_2015_edition() {
    check(
        r#"
//- /main.rs crate:main deps:alloc edition:2015
extern crate alloc as alloc_crate;
mod alloc;
mod sync;

//- /sync.rs
use alloc_crate::Arc;

//- /lib.rs crate:alloc
struct Arc;
"#,
        expect![[r#"
            crate
            alloc_crate: t
            sync: t

            crate::sync
            Arc: t v
        "#]],
    );
}

#[test]
fn reexport_across_crates() {
    check(
        r#"
//- /main.rs crate:main deps:test_crate
use test_crate::Baz;

//- /lib.rs crate:test_crate
pub use foo::Baz;
mod foo;

//- /foo.rs
pub struct Baz;
"#,
        expect![[r#"
            crate
            Baz: t v
        "#]],
    );
}

#[test]
fn values_dont_shadow_extern_crates() {
    check(
        r#"
//- /main.rs crate:main deps:foo
fn foo() {}
use foo::Bar;

//- /foo/lib.rs crate:foo
pub struct Bar;
"#,
        expect![[r#"
            crate
            Bar: t v
            foo: v
        "#]],
    );
}

#[test]
fn std_prelude_takes_precedence_above_core_prelude() {
    check(
        r#"
//- /main.rs crate:main deps:core,std
use {Foo, Bar};

//- /std.rs crate:std deps:core
#[prelude_import]
pub use self::prelude::*;
mod prelude {
    pub struct Foo;
    pub use core::prelude::Bar;
}

//- /core.rs crate:core
#[prelude_import]
pub use self::prelude::*;
mod prelude {
    pub struct Bar;
}
"#,
        expect![[r#"
            crate
            Bar: t v
            Foo: t v
        "#]],
    );
}

#[test]
fn cfg_not_test() {
    check(
        r#"
//- /main.rs crate:main deps:std
use {Foo, Bar, Baz};

//- /lib.rs crate:std
#[prelude_import]
pub use self::prelude::*;
mod prelude {
    #[cfg(test)]
    pub struct Foo;
    #[cfg(not(test))]
    pub struct Bar;
    #[cfg(all(not(any()), feature = "foo", feature = "bar", opt = "42"))]
    pub struct Baz;
}
"#,
        expect![[r#"
            crate
            Bar: t v
            Baz: _
            Foo: _
        "#]],
    );
}

#[test]
fn cfg_test() {
    check(
        r#"
//- /main.rs crate:main deps:std
use {Foo, Bar, Baz};

//- /lib.rs crate:std cfg:test,feature=foo,feature=bar,opt=42
#[prelude_import]
pub use self::prelude::*;
mod prelude {
    #[cfg(test)]
    pub struct Foo;
    #[cfg(not(test))]
    pub struct Bar;
    #[cfg(all(not(any()), feature = "foo", feature = "bar", opt = "42"))]
    pub struct Baz;
}
"#,
        expect![[r#"
            crate
            Bar: _
            Baz: t v
            Foo: t v
        "#]],
    );
}

#[test]
fn infer_multiple_namespace() {
    check(
        r#"
//- /main.rs
mod a {
    pub type T = ();
    pub use crate::b::*;
}

use crate::a::T;

mod b {
    pub const T: () = ();
}
"#,
        expect![[r#"
            crate
            T: t v
            a: t
            b: t

            crate::b
            T: v

            crate::a
            T: t v
        "#]],
    );
}

#[test]
fn underscore_import() {
    check(
        r#"
//- /main.rs
use tr::Tr as _;
use tr::Tr2 as _;

mod tr {
    pub trait Tr {}
    pub trait Tr2 {}
}
    "#,
        expect![[r#"
            crate
            _: t
            _: t
            tr: t

            crate::tr
            Tr: t
            Tr2: t
        "#]],
    );
}

#[test]
fn underscore_reexport() {
    check(
        r#"
//- /main.rs
mod tr {
    pub trait PubTr {}
    pub trait PrivTr {}
}
mod reex {
    use crate::tr::PrivTr as _;
    pub use crate::tr::PubTr as _;
}
use crate::reex::*;
    "#,
        expect![[r#"
            crate
            _: t
            reex: t
            tr: t

            crate::tr
            PrivTr: t
            PubTr: t

            crate::reex
            _: t
            _: t
        "#]],
    );
}

#[test]
fn underscore_pub_crate_reexport() {
    mark::check!(upgrade_underscore_visibility);
    check(
        r#"
//- /main.rs crate:main deps:lib
use lib::*;

//- /lib.rs crate:lib
use tr::Tr as _;
pub use tr::Tr as _;

mod tr {
    pub trait Tr {}
}
    "#,
        expect![[r#"
            crate
            _: t
        "#]],
    );
}

#[test]
fn underscore_nontrait() {
    check(
        r#"
//- /main.rs
mod m {
    pub struct Struct;
    pub enum Enum {}
    pub const CONST: () = ();
}
use crate::m::{Struct as _, Enum as _, CONST as _};
    "#,
        expect![[r#"
            crate
            m: t

            crate::m
            CONST: v
            Enum: t
            Struct: t v
        "#]],
    );
}

#[test]
fn underscore_name_conflict() {
    check(
        r#"
//- /main.rs
struct Tr;

use tr::Tr as _;

mod tr {
    pub trait Tr {}
}
    "#,
        expect![[r#"
            crate
            _: t
            Tr: t v
            tr: t

            crate::tr
            Tr: t
        "#]],
    );
}

#[test]
fn cfg_the_entire_crate() {
    check(
        r#"
//- /main.rs
#![cfg(never)]

pub struct S;
pub enum E {}
pub fn f() {}
    "#,
        expect![[r#"
            crate
        "#]],
    );
}
