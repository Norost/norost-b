#!/bin/bash

# Shamelessly stolen from https://eugene-babichenko.github.io/blog/2018/11/08/rustfmt-git-hook/

HAS_ISSUES=0

for file in $(git diff --name-only --staged --diff-filter=d); do
	# This sucks but rustfmt doesn't allow --check with stdin because ???? idfk
	if [[ "$file" =~ '.rs'$ ]]; then
		FMT_RESULT=$(diff <(git show ":$file") <(git show ":$file" | rustfmt --edition 2021))
		if [ "$FMT_RESULT" != "" ]; then
			echo "$file"
			HAS_ISSUES=1
		fi
	fi
done

if [ $HAS_ISSUES -eq 0 ]; then
    exit 0
fi

echo "Your code has formatting issues in the files listed above."
echo "Run \`cargo fmt\` and stage the changes."
exit 1

