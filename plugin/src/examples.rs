use include_dir::{include_dir, Dir};

static EXAMPLES_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/examples");

pub(crate) struct BuiltinExample {
    pub filename: String,
}

pub(crate) fn builtin_examples() -> Vec<BuiltinExample> {
    let mut filenames = EXAMPLES_DIR
        .files()
        .filter_map(|file| file.path().file_name())
        .filter_map(|name| name.to_str())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    filenames.sort();
    filenames
        .into_iter()
        .map(|filename| BuiltinExample { filename })
        .collect()
}

pub(crate) fn find_example_source(filename: &str) -> Option<&'static str> {
    EXAMPLES_DIR.get_file(filename).and_then(|file| file.contents_utf8())
}
