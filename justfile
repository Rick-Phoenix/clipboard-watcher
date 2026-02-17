check-apple:
    cargo check --tests --target x86_64-apple-darwin

test:
    cargo test -- --nocapture --test-threads=1

update-changelog version:
    #!/usr/bin/env bash

    echo "Generating changelog..."
    git cliff --tag {{ version }} -o "CHANGELOG.md" -- 2697dee6dd799191426ccf58f18e87451333580d..HEAD

    if [[  -z $(git status --porcelain "CHANGELOG.md") ]]; then
      echo "Changelog unchanged."
    else
      git add "CHANGELOG.md"

      echo "Committing the new changelog"
      git commit -m "updated changelog"
    fi

release-test version="patch":
    cargo release {{ version }}

release-exec version: (update-changelog version)
    cargo release {{ version }} --execute
