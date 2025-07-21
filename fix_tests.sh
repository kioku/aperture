#\!/bin/bash

# List of files that need fixing
files=(
    "tests/batch_processing_integration_tests.rs"
    "tests/cache_models_tests.rs"
    "tests/engine_executor_tests.rs"
    "tests/engine_loader_tests.rs"
    "tests/experimental_flags_tests.rs"
    "tests/response_cache_integration_tests.rs"
    "tests/security_integration_tests.rs"
    "tests/agent_manifest_tests.rs"
    "tests/command_syntax_integration_tests.rs"
    "tests/engine_generator_tests.rs"
)

for file in "${files[@]}"; do
    # Check if file exists
    if [ -f "$file" ]; then
        # Add skipped_endpoints field after security_schemes
        sed -i '' '/security_schemes: .*,$/s/$/\n        skipped_endpoints: vec\![],/' "$file"
        echo "Fixed: $file"
    fi
done
