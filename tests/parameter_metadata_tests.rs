use aperture_cli::agent::{
    generate_capability_manifest, generate_capability_manifest_from_openapi,
};
use aperture_cli::engine::generator::generate_command_tree_with_flags;
use aperture_cli::spec::SpecTransformer;
use serde_json::{json, Value};

fn spec() -> openapiv3::OpenAPI {
    serde_yaml::from_str(
        r"
openapi: 3.0.3
info: {title: Items, version: '1'}
paths:
  /items/{id}:
    get:
      operationId: listItems
      tags: [items]
      parameters:
        - name: id
          in: path
          required: true
          description: Item identifier.
          schema: {type: integer, format: int64, minimum: 1, maximum: 99, default: 2, description: Ignored description.}
        - name: limit
          in: query
          schema: {$ref: '#/components/schemas/Limit'}
        - name: kind
          in: header
          schema: {type: string, enum: [small, large], default: small, description: Item size class.}
        - name: q
          in: query
          schema: {type: string, minLength: 1, maxLength: 200, pattern: '^[a-z]+$'}
        - name: ratio
          in: query
          schema: {type: number, format: double, minimum: 0.5, maximum: 2.5, exclusiveMinimum: true, exclusiveMaximum: true, enum: [1.0, 2.0]}
        - name: plain
          in: query
          schema: {type: string}
      responses: {'200': {description: ok}}
components:
  schemas:
    Limit: {type: integer, format: int32, minimum: 1, maximum: 50, default: 10, enum: [10, 20], description: Schema page size.}
",
    )
    .unwrap()
}

fn parameters(manifest: &str) -> Value {
    let manifest: Value = serde_json::from_str(manifest).unwrap();
    manifest["commands"]["items"][0]["parameters"].clone()
}

#[test]
fn manifests_preserve_parameter_metadata() {
    let spec = spec();
    let cached = SpecTransformer::new().transform("items", &spec).unwrap();
    let manifests = [
        generate_capability_manifest(&cached, None).unwrap(),
        generate_capability_manifest_from_openapi("items", &spec, &cached, None).unwrap(),
    ];
    for manifest in manifests {
        let params = parameters(&manifest);
        assert_eq!(params[0]["description"], "Item identifier.");
        assert_eq!(params[0]["format"], "int64");
        assert_eq!(params[1]["description"], "Schema page size.");
        assert_eq!(params[1]["minimum"], 1);
        assert_eq!(params[1]["maximum"], 50);
        assert_eq!(params[1]["format"], "int32");
        assert_eq!(params[1]["default_value"], "10");
        assert_eq!(params[1]["enum_values"], json!([10, 20]));
        assert_eq!(params[2]["description"], "Item size class.");
        assert_eq!(params[2]["enum_values"], json!(["small", "large"]));
        assert_eq!(params[3]["min_length"], 1);
        assert_eq!(params[3]["max_length"], 200);
        assert_eq!(params[3]["pattern"], "^[a-z]+$");
        assert_eq!(params[4]["format"], "double");
        assert_eq!(params[4]["minimum"], 0.5);
        assert_eq!(params[4]["maximum"], 2.5);
        assert_eq!(params[4]["exclusive_minimum"], true);
        assert_eq!(params[4]["exclusive_maximum"], true);
        assert_eq!(params[4]["enum_values"], json!([1.0, 2.0]));
        assert_eq!(
            params[5],
            json!({"name": "plain", "location": "query", "required": false, "param_type": "string", "description": null})
        );
    }
}

#[test]
fn help_displays_metadata_without_enforcing_constraints() {
    let spec = spec();
    let cached = SpecTransformer::new().transform("items", &spec).unwrap();
    for positional in [false, true] {
        let mut tree = generate_command_tree_with_flags(&cached, positional);
        let operation = tree
            .find_subcommand_mut("items")
            .unwrap()
            .find_subcommand_mut("list-items")
            .unwrap();
        let help = operation.clone().render_long_help().to_string();
        assert_eq!(
            operation
                .get_arguments()
                .find(|arg| arg.get_id() == "plain")
                .unwrap()
                .get_help()
                .unwrap()
                .to_string(),
            "Query plain parameter"
        );
        for expected in [
            "Item identifier.",
            "minimum: 1",
            "maximum: 99",
            "default: 2",
            "Schema page size.",
            "maximum: 50",
            "default: 10",
            "Item size class.",
            "possible values: small, large",
            "default: small",
            "min length: 1",
            "max length: 200",
            "pattern: ^[a-z]+$",
            "exclusive minimum: 0.5",
            "exclusive maximum: 2.5",
            "Query plain parameter",
        ] {
            assert!(help.contains(expected), "missing {expected:?} in {help}");
        }
        assert!(!help.contains("Ignored description."));
        // Metadata is informational: pre-send validation is a separate change.
        if !positional {
            tree.try_get_matches_from([
                "api",
                "items",
                "list-items",
                "--id",
                "999",
                "--kind",
                "huge",
            ])
            .unwrap();
        }
    }
}

#[test]
fn help_preserves_unconstrained_parameters_in_each_location() {
    for positional in [false, true] {
        for (location, expected) in [
            (
                "path",
                if positional {
                    "id parameter"
                } else {
                    "Path parameter: id"
                },
            ),
            ("query", "Query id parameter"),
            ("header", "Header id parameter"),
            ("cookie", "id parameter"),
        ] {
            let mut source = serde_json::to_value(spec()).unwrap();
            source["paths"]["/items/{id}"]["get"]["parameters"] = json!([
                {"name": "id", "in": location, "required": true, "schema": {"type": "string"}}
            ]);
            let spec = serde_json::from_value(source).unwrap();
            let cached = SpecTransformer::new().transform("items", &spec).unwrap();
            let tree = generate_command_tree_with_flags(&cached, positional);
            let operation = tree
                .find_subcommand("items")
                .unwrap()
                .find_subcommand("list-items")
                .unwrap();
            let arg = operation
                .get_arguments()
                .find(|arg| arg.get_id() == "id")
                .unwrap();
            assert_eq!(arg.get_help().unwrap().to_string(), expected);
        }
    }
}

#[test]
fn help_shows_schema_fallback_and_length_bounds_in_each_location() {
    for location in ["path", "query", "header", "cookie"] {
        let mut source = serde_json::to_value(spec()).unwrap();
        source["paths"]["/items/{id}"]["get"]["parameters"] = json!([
            {"name": "id", "in": location, "required": true,
             "schema": {"type": "string", "description": "Item code.",
                        "minLength": 2, "maxLength": 8, "default": "ab", "enum": ["ab", "cd"]}}
        ]);
        let spec = serde_json::from_value(source).unwrap();
        let cached = SpecTransformer::new().transform("items", &spec).unwrap();
        let tree = generate_command_tree_with_flags(&cached, false);
        let operation = tree
            .find_subcommand("items")
            .unwrap()
            .find_subcommand("list-items")
            .unwrap();
        let arg = operation
            .get_arguments()
            .find(|arg| arg.get_id() == "id")
            .unwrap();
        assert_eq!(
            arg.get_help().unwrap().to_string(),
            "Item code. [min length: 2, max length: 8, possible values: ab, cd, default: ab]"
        );
    }
}

#[test]
fn all_of_reference_wrappers_preserve_metadata() {
    let mut source = serde_json::to_value(spec()).unwrap();
    source["components"]["schemas"]["Code"] = json!({
        "type": "string", "description": "Referenced code.",
        "minLength": 2, "maxLength": 8, "enum": ["ab", "cd"], "default": "ab"
    });
    source["paths"]["/items/{id}"]["get"]["parameters"] = json!([
        {"name": "id", "in": "path", "required": true,
         "schema": {"description": "Wrapper description.",
                    "allOf": [{"$ref": "#/components/schemas/Code"}]}}
    ]);
    let spec = serde_json::from_value(source).unwrap();
    let cached = SpecTransformer::new().transform("items", &spec).unwrap();
    for manifest in [
        generate_capability_manifest(&cached, None).unwrap(),
        generate_capability_manifest_from_openapi("items", &spec, &cached, None).unwrap(),
    ] {
        let params = parameters(&manifest);
        assert_eq!(params[0]["description"], "Wrapper description.");
        assert_eq!(params[0]["min_length"], 2);
        assert_eq!(params[0]["max_length"], 8);
        assert_eq!(params[0]["enum_values"], json!(["ab", "cd"]));
        assert_eq!(params[0]["default_value"], "\"ab\"");
    }
    let mut tree = generate_command_tree_with_flags(&cached, false);
    let help = tree
        .find_subcommand_mut("items")
        .unwrap()
        .find_subcommand_mut("list-items")
        .unwrap()
        .render_long_help()
        .to_string();
    assert!(help.contains("Wrapper description."));
    assert!(help.contains("min length: 2"));
    assert!(help.contains("possible values: ab, cd"));
}

#[test]
fn boolean_metadata_does_not_change_flag_parsing() {
    for location in ["query", "header"] {
        let mut source = serde_json::to_value(spec()).unwrap();
        source["paths"]["/items/{id}"]["get"]["parameters"] = json!([
            {"name": "active", "in": location, "description": "Active items.",
             "schema": {"type": "boolean", "enum": [true, false], "default": false}}
        ]);
        let spec = serde_json::from_value(source).unwrap();
        let cached = SpecTransformer::new().transform("items", &spec).unwrap();
        for manifest in [
            generate_capability_manifest(&cached, None).unwrap(),
            generate_capability_manifest_from_openapi("items", &spec, &cached, None).unwrap(),
        ] {
            let params = parameters(&manifest);
            assert_eq!(params[0]["enum_values"], json!([true, false]));
            assert_eq!(params[0]["default_value"], "false");
        }
        let tree = generate_command_tree_with_flags(&cached, false);
        let arg = tree
            .find_subcommand("items")
            .unwrap()
            .find_subcommand("list-items")
            .unwrap()
            .get_arguments()
            .find(|arg| arg.get_id() == "active")
            .unwrap();
        assert_eq!(
            arg.get_help().unwrap().to_string(),
            "Active items. [possible values: true, false, default: false]"
        );
        for (args, expected) in [
            (vec![], None),
            (vec!["--active"], Some(true)),
            (vec!["--active=true"], Some(true)),
            (vec!["--active=false"], Some(false)),
        ] {
            let matches = tree
                .clone()
                .try_get_matches_from(["api", "items", "list-items"].into_iter().chain(args))
                .unwrap();
            let operation = matches.subcommand().unwrap().1.subcommand().unwrap().1;
            assert_eq!(operation.get_one::<bool>("active").copied(), expected);
        }
        for args in [
            vec!["--active=foo"],
            vec!["--active="],
            vec!["--active= "],
            vec!["--active=none"],
            vec!["--active=true", "--active=false"],
            vec!["--unknown"],
        ] {
            assert!(tree
                .clone()
                .try_get_matches_from(["api", "items", "list-items"].into_iter().chain(args))
                .is_err());
        }
    }
}

#[test]
fn description_presence_takes_precedence_even_when_empty() {
    for description in ["", " ", "Canonical description."] {
        let mut source = serde_json::to_value(spec()).unwrap();
        source["paths"]["/items/{id}"]["get"]["parameters"] = json!([
            {"name": "id", "in": "path", "required": true, "description": description,
             "schema": {"type": "string", "description": "Fallback.",
                        "nullable": true, "enum": [null, "ab", "ab", "none", ""]}}
        ]);
        let spec = serde_json::from_value(source).unwrap();
        let cached = SpecTransformer::new().transform("items", &spec).unwrap();
        for manifest in [
            generate_capability_manifest(&cached, None).unwrap(),
            generate_capability_manifest_from_openapi("items", &spec, &cached, None).unwrap(),
        ] {
            let params = parameters(&manifest);
            assert_eq!(params[0]["description"], description);
            assert_eq!(
                params[0]["enum_values"],
                json!([null, "ab", "ab", "none", ""])
            );
        }
        let tree = generate_command_tree_with_flags(&cached, false);
        let operation = tree
            .find_subcommand("items")
            .unwrap()
            .find_subcommand("list-items")
            .unwrap();
        let help = operation
            .get_arguments()
            .find(|arg| arg.get_id() == "id")
            .unwrap()
            .get_help()
            .unwrap()
            .to_string();
        assert_eq!(
            help,
            format!("{description} [possible values: null, ab, ab, none, ]")
        );
        for value in ["", " ", "none", "unknown"] {
            // No enum validation is introduced, even for empty or sentinel strings.
            assert!(tree
                .clone()
                .try_get_matches_from(["api", "items", "list-items", "--id", value])
                .is_ok());
        }
    }
}

#[test]
fn boolean_path_flags_keep_existing_presence_semantics() {
    let mut source = serde_json::to_value(spec()).unwrap();
    source["paths"]["/items/{id}"]["get"]["parameters"] = json!([
        {"name": "id", "in": "path", "required": true,
         "schema": {"type": "boolean", "description": "Boolean identifier.", "default": true}}
    ]);
    let spec = serde_json::from_value(source).unwrap();
    let cached = SpecTransformer::new().transform("items", &spec).unwrap();
    for positional in [false, true] {
        let tree = generate_command_tree_with_flags(&cached, positional);
        let arg = tree
            .find_subcommand("items")
            .unwrap()
            .find_subcommand("list-items")
            .unwrap()
            .get_arguments()
            .find(|arg| arg.get_id() == "id")
            .unwrap();
        assert_eq!(
            arg.get_help().unwrap().to_string(),
            "Boolean identifier. [default: true]"
        );
        for (args, expected) in [(vec![], false), (vec!["--id"], true)] {
            let matches = tree
                .clone()
                .try_get_matches_from(["api", "items", "list-items"].into_iter().chain(args))
                .unwrap();
            let operation = matches.subcommand().unwrap().1.subcommand().unwrap().1;
            // Declared schema defaults are display metadata, not clap defaults.
            assert_eq!(operation.get_one::<bool>("id"), Some(&expected));
        }
        assert!(tree
            .try_get_matches_from(["api", "items", "list-items", "--id=false"])
            .is_err());
    }
}
