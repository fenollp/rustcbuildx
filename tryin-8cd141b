#!/bin/bash -eu
# trash target /tmp/global.lock >/dev/null 2>&1; shellcheck ./tryin.sh && PROFILE=debug CARGO_HOME=$HOME/.cargo CARGO_TARGET_DIR=$PWD/target RUSTC_WRAPPER=$PWD/tryin.sh cargo build --locked --frozen --offline --all-targets --all-features

if [[ "CARGO_MANIFEST_DIR=${CARGO_MANIFEST_DIR:-}" == "${RUSTCBUILDX_DEBUG:-}" ]]; then
	RUSTCBUILDX_DEBUG=1
fi

if [[ "${RUSTCBUILDX_DEBUG:-}" == '1' ]]; then
	set -x
fi

_rustc() {
	if [[ "${RUSTCBUILDX_DEBUG:-}" == '1' ]]; then
		until (set -o noclobber; echo >/tmp/global.lock) >/dev/null 2>&1; do
			[[ "$(( "$(date +%s)" - "$(stat -c %Y /tmp/global.lock)" ))" -ge 60 ]] && return 4
			sleep .5
		done
	fi

	local args=()

	local crate_name=''
	local crate_type=''
	local externs=()
	local extra_filename=''
	local incremental=''
	local input=''
	local out_dir=''

	local key=''; local val=''; local pair=''
	for arg in "$@"; do
		case "$pair" in
		'' ) pair=S; key=$arg; [[ "$arg" == '--crate-name' ]] || return 4; continue ;;
		'E') pair=S; key=$arg; val=''   ;; # start
		'S') pair=E;           val=$arg ;; # end
		esac
		if [[ "$pair $val" == 'S ' ]] && [[ "$arg" =~ ^--.+=.+ ]]; then
			pair=E; key=${arg%=*}; val=${arg#*=}
		fi

		case "$key" in /*|src/lib.rs|src/main.rs|*/src/lib.rs|*/src/main.rs)
			[[ "$input" != '' ]] && return 4
			input=$key
			pair=E; key=''; val=''
			# For e.g. $HOME/.cargo/registry/src/github.com-1ecc6299db9ec823/ahash-0.7.6/./build.rs
			# shellcheck disable=SC2001
			input=$(sed 's%/[.]/%/%g' <<<"$input")
			continue ;;
		esac

		if [[ "$pair $key $val" == 'S --test ' ]]; then
			[[ "$crate_type" != '' ]] && return 4
			crate_type='test' # Not a real `--crate-type`
			pair=E; key=''; val=''
			args+=('--test')
			continue
		fi

		# FIXME: revert
		case "$key $val" in
		# strips out local config for now
		'-C link-arg=-fuse-ld=/usr/local/bin/mold')
			pair=E; key=''; val=''
			continue ;;
		'-C linker=/usr/bin/clang')
			pair=E; key=''; val=''
			continue ;;
		'--json diagnostic-rendered-ansi,artifacts,future-incompat')
			if [[ "${RUSTCBUILDX_DEBUG:-}" == '1' ]]; then
				# remove coloring in output for readability during debug
				val='artifacts,future-incompat'
			fi
			;;
		esac

		[[ "$val" == '' ]] && continue

		case "$key $val" in
		'-C extra-filename='*)
			[[ "$extra_filename" != '' ]] && return 4
			extra_filename=${val#extra-filename=}
			;;

		'-C incremental='*)
			[[ "$incremental" != '' ]] && return 4
			incremental=${val#incremental=}
			;;

		'-L dependency='*)
			case "${val#dependency=}" in /*) ;; *) val=dependency=$PWD/${val#dependency=} ;; esac
			;;

		'--crate-name '*)
			[[ "$crate_name" != '' ]] && return 4
			crate_name=$val
			;;

		'--crate-type '*)
			[[ "$crate_type" != '' ]] && return 4
			case "$val" in bin|lib|proc-macro) ;; *) return 4;; esac
			crate_type=$val
			;;

		'--extern '*)
			# Sysroot crates (e.g. https://doc.rust-lang.org/proc_macro)
			case "$val" in alloc|core|proc_macro|std|test) continue ;; esac
			local extern=${val#*=}
			# NOTE:
			# https://github.com/rust-lang/cargo/issues/9661
			# https://github.com/dtolnay/cxx/blob/83d9d43892d9fe67dd031e4115ae38d0ef3c4712/gen/build/src/target.rs#L10
			# https://github.com/rust-lang/cargo/issues/6100
			# This doesn't always verify: case "$extern" in "$deps_path"/*) ;; *) return 4 ;; esac
			# because $CARGO_TARGET_DIR is sometimes set to $PWD/target which is sometimes $HOME/.cargo/registry/src/github.com-1ecc6299db9ec823/anstyle-parse-0.1.1
			# So we can't do: externs+=("${extern#"$deps_path"/}")
			# Anyway the goal is simply to just extract libutf8parse-03cddaef72c90e73.rmeta from $HOME/wefwefwef/buildxargs.git/target/debug/deps/libutf8parse-03cddaef72c90e73.rmeta
			# So let's just do that!
			externs+=("$(basename "$extern")")
			;;

		'--out-dir '*)
			[[ "$out_dir" != '' ]] && return 4
			out_dir=$val
			case "$out_dir" in /*) ;; *) out_dir=$PWD/$out_dir ;; esac # TODO: decide whether $PWD is an issue. Maybe CARGO_TARGET_DIR can help?
			val=$out_dir
			;;
		esac

		args+=("$key" "$val")
	done

	[[ "$crate_name" == '' ]] && return 4
	[[ "$crate_type" == '' ]] && return 4
	[[ "$extra_filename" == '' ]] && return 4
	# [[ "$incremental" == '' ]] && return 4 MAY be unset: only set on last calls
	[[ "$input" == '' ]] && return 4
	[[ "$out_dir" == '' ]] && return 4

	# https://github.com/rust-lang/cargo/issues/12099
	# Sometimes, a proc-macro crate that depends on sysroot crate `proc_macro` is missing `--extern proc_macro` rustc flag.
	# So add it here or it won't compile. (e.g. openssl-macros-0.1.0-024d32b3f7af0a4f)
	# {"message":"unresolved import `proc_macro`","code":{"code":"E0432","explanation":"An import was unresolved.\n\nErroneous code example:\n\n```compile_fail,E0432\nuse something::Foo; // error: unresolved import `something::Foo`.\n```\n\nIn Rust 2015, paths in `use` statements are relative to the crate root. To\nimport items relative to the current and parent modules, use the `self::` and\n`super::` prefixes, respectively.\n\nIn Rust 2018 or later, paths in `use` statements are relative to the current\nmodule unless they begin with the name of a crate or a literal `crate::`, in\nwhich case they start from the crate root. As in Rust 2015 code, the `self::`\nand `super::` prefixes refer to the current and parent modules respectively.\n\nAlso verify that you didn't misspell the import name and that the import exists\nin the module from where you tried to import it. Example:\n\n```\nuse self::something::Foo; // Ok.\n\nmod something {\n    pub struct Foo;\n}\n# fn main() {}\n```\n\nIf you tried to use a module from an external crate and are using Rust 2015,\nyou may have missed the `extern crate` declaration (which is usually placed in\nthe crate root):\n\n```edition2015\nextern crate core; // Required to use the `core` crate in Rust 2015.\n\nuse core::any;\n# fn main() {}\n```\n\nSince Rust 2018 the `extern crate` declaration is not required and\nyou can instead just `use` it:\n\n```edition2018\nuse core::any; // No extern crate required in Rust 2018.\n# fn main() {}\n```\n"},"level":"error","spans":[{"file_name":"/home/pete/.cargo/registry/src/github.com-1ecc6299db9ec823/openssl-macros-0.1.0/src/lib.rs","byte_start":4,"byte_end":14,"line_start":1,"line_end":1,"column_start":5,"column_end":15,"is_primary":true,"text":[{"text":"use proc_macro::TokenStream;","highlight_start":5,"highlight_end":15}],"label":"use of undeclared crate or module `proc_macro`","suggested_replacement":null,"suggestion_applicability":null,"expansion":null}],"children":[{"message":"there is a crate or module with a similar name","code":null,"level":"help","spans":[{"file_name":"/home/pete/.cargo/registry/src/github.com-1ecc6299db9ec823/openssl-macros-0.1.0/src/lib.rs","byte_start":4,"byte_end":14,"line_start":1,"line_end":1,"column_start":5,"column_end":15,"is_primary":true,"text":[{"text":"use proc_macro::TokenStream;","highlight_start":5,"highlight_end":15}],"label":null,"suggested_replacement":"proc_macro2","suggestion_applicability":"MaybeIncorrect","expansion":null}],"children":[],"rendered":null}],"rendered":"error[E0432]: unresolved import `proc_macro`\n --> /home/pete/.cargo/registry/src/github.com-1ecc6299db9ec823/openssl-macros-0.1.0/src/lib.rs:1:5\n  |\n1 | use proc_macro::TokenStream;\n  |     ^^^^^^^^^^ use of undeclared crate or module `proc_macro`\n  |\nhelp: there is a crate or module with a similar name\n  |\n1 | use proc_macro2::TokenStream;\n  |     ~~~~~~~~~~~\n\n"}
	if [[ "$crate_type" == 'proc-macro' ]]; then
		case "${args[*]}" in
		*' --extern proc_macro '*) ;;
		*' --extern=proc_macro '*) ;;
		*) args+=('--extern' 'proc_macro') ;;
		esac
	fi

	# Can't rely on $PWD nor $CARGO_TARGET_DIR because `cargo` changes them.
	# Out dir though...
	# --out-dir "$CARGO_TARGET_DIR/$PROFILE"/build/rustix-2a01a00f5bdd1924
	# --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps
	local target_path=''
	case "$out_dir" in
	*/deps) target_path=${out_dir%/deps} ;;
	*/build/*) target_path=${out_dir%/build/*} ;;
	*) return 4 ;;
	esac
	mkdir -p "$target_path"/deps

	local crate_out=''
	if [[ "${OUT_DIR:-}" =~ /out$ ]]; then
		crate_out=$OUT_DIR # NOTE: not $out_dir
	fi

	local full_crate_id
	full_crate_id=$crate_type-$crate_name$extra_filename

	# https://github.com/rust-lang/cargo/issues/12059
	local all_externs=()
	local externs_prefix=$target_path/externs_
	local crate_externs=$externs_prefix$crate_name$extra_filename
	if ! [[ -s "$crate_externs" ]]; then
		local ext=''
		case "$crate_type" in
		lib)        ext=rmeta ;;
		bin)        ext=rlib ;;
		test)       ext=rlib ;;
		proc-macro) ext=rlib
			touch "${crate_externs}_proc-macro" ;; # This way crates that depend on this know they must require it as .so
		*) return 4 ;;
		esac

		# shellcheck disable=SC2207
		IFS=$'\n' externs=($(sort -u <<<"${externs[*]}")); unset IFS
		local short_externs=()
		for extern in "${externs[@]}"; do
			all_externs+=("$extern")

			case "$extern" in lib*) ;; *) return 4 ;; esac
			extern=${extern#lib}
			case "$extern" in
			*.rlib) extern=${extern%.rlib} ;;
			*.rmeta) extern=${extern%.rmeta} ;;
			*.so) extern=${extern%.so} ;;
			*) return 4 ;;
			esac
			short_externs+=("$extern")

			local extern_crate_externs=$externs_prefix$extern
			if [[ -s "$extern_crate_externs" ]]; then
				while read -r transitive; do
					[[ "$transitive" == '' ]] && return 4
					if [[ -f "$externs_prefix${transitive}_proc-macro" ]]; then
						all_externs+=("lib$transitive.so")
					else
						all_externs+=("lib$transitive.$ext")
					fi
					short_externs+=("$transitive")
				done <"$extern_crate_externs"
			fi
		done
		# shellcheck disable=SC2207
		IFS=$'\n' all_externs=($(sort -u <<<"${all_externs[*]}")); unset IFS
		if [[ ${#short_externs[@]} -ne 0 ]]; then
			# shellcheck disable=SC2207
			IFS=$'\n' short_externs=($(sort -u <<<"${short_externs[*]}")); unset IFS
			printf "%s\n" "${short_externs[@]}" >"$crate_externs"
		fi
	fi
	if [[ "${RUSTCBUILDX_DEBUG:-}" == '1' ]]; then
		if [[ -s "$crate_externs" ]]; then
			echo "$crate_externs" >&2
			cat  "$crate_externs" >&2 || true
		fi
	fi

	mkdir -p "$out_dir"
	[[ "$incremental" != '' ]] && mkdir -p "$incremental"

	local input_mount_name input_mount_target rustc_stage
	case "$input" in
	src/lib.rs)
		input_mount_name=''
		input_mount_target=''
		rustc_stage=final-$full_crate_id
		;;
	src/main.rs)
		input_mount_name=''
		input_mount_target=''
		rustc_stage=final-$full_crate_id
		;;
	*/build.rs)
		input_mount_name=input_build_rs--$(basename "${input%/build.rs}")
		input_mount_target=${input%/build.rs}
		rustc_stage=build_rs-$full_crate_id
		;;
	*/src/lib.rs)
		input_mount_name=input_src_lib_rs--$(basename "${input%/src/lib.rs}")
		input_mount_target=${input%/src/lib.rs}
		rustc_stage=src_lib_rs-$full_crate_id
		;;
	*/lib.rs) # This ordering...
		# e.g. $HOME/.cargo/registry/src/github.com-1ecc6299db9ec823/fnv-1.0.7/lib.rs
		input_mount_name=input_lib_rs--$(basename "${input%/lib.rs}")
		input_mount_target=${input%/lib.rs}
		rustc_stage=lib_rs-$full_crate_id
		;;
	*/src/*.rs) # ...matters (input_mount_target)
		# e.g. input=$HOME/.cargo/registry/src/github.com-1ecc6299db9ec823/untrusted-0.7.1/src/untrusted.rs
		input_mount_target=$(dirname "$input")
		input_mount_target=${input_mount_target%/src}
		# ^ instead of: input_mount_target=${input%/src/*.rs}
		input_mount_name=input_src__rs--$(basename "$input_mount_target")
		rustc_stage=src__rs-$full_crate_id
		;;
	*) return 4 ;;
	esac
	[[ "$input_mount_target" == "$HOME/.cargo/registry" ]] && return 4

	local backslash="\\"

	local incremental_stage=incremental$extra_filename
	local out_stage=out$extra_filename
	local stdio_stage=stdio$extra_filename
	local toolchain_stage=''
	if [[ "$input_mount_target" != '' ]] && [[ -s "$input_mount_target"/rust-toolchain ]]; then
		# https://rust-lang.github.io/rustup/overrides.html
		# NOTE: without this, the crate's rust-toolchain gets installed and used and (for the mentioned crate)
		#   fails due to (yet)unknown rustc CLI arg: `error: Unrecognized option: 'diagnostic-width'`
		# e.g. https://github.com/xacrimon/dashmap/blob/v5.4.0/rust-toolchain
		toolchain_stage=toolchain$extra_filename
	fi

	# RUSTCBUILDX_DOCKER_IMAGE MUST start with docker-image:// and image MUST be available on DOCKER_HOST e.g.:
	# RUSTCBUILDX_DOCKER_IMAGE=docker-image://rustc_with_libs
	# DOCKER_HOST=ssh://oomphy docker buildx build -t rustc_with_libs - <<EOF
	# FROM docker.io/library/rust:1.69.0-slim-bookworm@sha256:8bdd28ef184d85c9b4932586af6280732780e806e5f452065420f2e783323ca3
	# RUN set -eux && apt update && apt install -y libpq-dev libssl3
	# EOF
	RUSTCBUILDX_DOCKER_IMAGE=${RUSTCBUILDX_DOCKER_IMAGE:-docker-image://docker.io/library/rust:1.69.0-slim@sha256:8b85a8a6bf7ed968e24bab2eae6f390d2c9c8dbed791d3547fef584000f48f9e} # rustc 1.69.0 (84c898d65 2023-04-16)
	RUSTCBUILDX_DOCKER_SYNTAX=${RUSTCBUILDX_DOCKER_SYNTAX:-docker.io/docker/dockerfile:1@sha256:39b85bbfa7536a5feceb7372a0817649ecb2724562a38360f4d6a7782a409b14}


	local dockerfile=$target_path/${extra_filename#-}.Dockerfile
	printf '# syntax=%s\n' "$RUSTCBUILDX_DOCKER_SYNTAX" >"$dockerfile"

	if [[ "$toolchain_stage" != '' ]]; then
		cat <<EOF >>"$dockerfile"
FROM rust AS $toolchain_stage
RUN rustup default | cut -d- -f1 >/rustup-toolchain
EOF
	fi

	cat <<EOF >>"$dockerfile"
FROM rust AS $rustc_stage
WORKDIR $out_dir
EOF

	if [[ "$incremental" != '' ]]; then
		cat <<EOF >>"$dockerfile"
WORKDIR $incremental
EOF
	fi

	local cwd=''
	if [[ "$input" =~ ^[^/]+(/[^/]+){0,2}.rs$ ]]; then
		cwd=$(mktemp -d) # TODO: use tmpfs when on *NIX
		if [[ -d "$PWD"/.git ]]; then
			while read -r f; do
				mkdir -p "$cwd/$(dirname "$f")"
				cp "$f" "$cwd/$f"
			done < <(git ls-files "$PWD" | sort)
		else
			while read -r f; do
				f=${f#"$PWD"}
				mkdir -p "$cwd/$(dirname "$f")"
				cp "$f" "$cwd/$f"
			done < <(find "$PWD" -type f | sort)
		fi
		cat <<EOF >>"$dockerfile"
WORKDIR $PWD
COPY --from=cwd / .
RUN $backslash
EOF
	else
		[[ "$input_mount_name" == '' ]] && return 4
		cat <<EOF >>"$dockerfile"
WORKDIR $PWD
RUN $backslash
  --mount=type=bind,from=$input_mount_name,target=$input_mount_target $backslash
EOF
	fi

	crate_out_name() {
		local name=$1; shift
		# name=/home/pete/wefwefwef/buildxargs.git/target/debug/build/quote-adce79444856d618/out
		name=${name##*-}
		# name=adce79444856d618/out
		name=${name%%/out}
		# name=adce79444856d618
		echo "crate_out-$name"
	}

	if [[ "$crate_out" != '' ]]; then
		cat <<EOF >>"$dockerfile"
  --mount=type=bind,from=$(crate_out_name "$crate_out"),target=$crate_out $backslash
EOF
	fi

	if [[ "$toolchain_stage" != '' ]]; then
		cat <<EOF >>"$dockerfile"
  --mount=type=bind,from=$toolchain_stage,source=/rustup-toolchain,target=/rustup-toolchain $backslash
EOF
	fi

	local bakefiles=()
	for extern in "${all_externs[@]}"; do
		local extern_bakefile=$extern
		# extern_bakefile=libstrsim-8ed1051e7e58e636.rlib
		extern_bakefile="${extern_bakefile##lib}"
		# extern_bakefile=strsim-8ed1051e7e58e636.rlib
		extern_bakefile="${extern_bakefile%%.*}"
		# extern_bakefile=strsim-8ed1051e7e58e636
		local extern_bakefile_stage=$extern_bakefile
		extern_bakefile_stage="${extern_bakefile_stage##*-}"
		# extern_bakefile_stage=8ed1051e7e58e636
		extern_bakefile_stage="out-$extern_bakefile_stage"
		# extern_bakefile_stage=out-8ed1051e7e58e636
		extern_bakefile="$target_path/$extern_bakefile.hcl"
		# extern_bakefile=_target/debug/strsim-8ed1051e7e58e636.hcl
		# cat "$extern_bakefile"
		# echo mount from:"$extern_bakefile_stage" source:"/$extern" target:"$target_path/deps/$extern"
		bakefiles+=("$extern_bakefile")
		cat <<EOF >>"$dockerfile"
  --mount=type=bind,from=$extern_bakefile_stage,source=/$extern,target=$target_path/deps/$extern $backslash
EOF
	done

	# shellcheck disable=SC2129,SC2001
	echo "    export LD_LIBRARY_PATH='$(sed "s%'%%g" <<<"${LD_LIBRARY_PATH:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-crates
	# shellcheck disable=SC2001
	echo "    export CARGO='$(sed "s%'%%g" <<<"${CARGO:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_MANIFEST_DIR='$(sed "s%'%%g" <<<"${CARGO_MANIFEST_DIR:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_PKG_VERSION='$(sed "s%'%%g" <<<"${CARGO_PKG_VERSION:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_PKG_VERSION_MAJOR='$(sed "s%'%%g" <<<"${CARGO_PKG_VERSION_MAJOR:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_PKG_VERSION_MINOR='$(sed "s%'%%g" <<<"${CARGO_PKG_VERSION_MINOR:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_PKG_VERSION_PATCH='$(sed "s%'%%g" <<<"${CARGO_PKG_VERSION_PATCH:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_PKG_VERSION_PRE='$(sed "s%'%%g" <<<"${CARGO_PKG_VERSION_PRE:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_PKG_AUTHORS='$(sed "s%'%%g" <<<"${CARGO_PKG_AUTHORS:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_PKG_NAME='$(sed "s%'%%g" <<<"${CARGO_PKG_NAME:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_PKG_DESCRIPTION='$(sed "s%'%%g" <<<"${CARGO_PKG_DESCRIPTION:-}" | tr -d '\n' | tr -d '`')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_PKG_HOMEPAGE='$(sed "s%'%%g" <<<"${CARGO_PKG_HOMEPAGE:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_PKG_REPOSITORY='$(sed "s%'%%g" <<<"${CARGO_PKG_REPOSITORY:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_PKG_LICENSE='$(sed "s%'%%g" <<<"${CARGO_PKG_LICENSE:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_PKG_LICENSE_FILE='$(sed "s%'%%g" <<<"${CARGO_PKG_LICENSE_FILE:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_PKG_RUST_VERSION='$(sed "s%'%%g" <<<"${CARGO_PKG_RUST_VERSION:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_CRATE_NAME='$(sed "s%'%%g" <<<"${CARGO_CRATE_NAME:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# shellcheck disable=SC2001
	echo "    export CARGO_BIN_NAME='$(sed "s%'%%g" <<<"${CARGO_BIN_NAME:-}" | tr -d '\n')' && $backslash" >>"$dockerfile"
	# TODO: allow additional envs to be passed as RUSTCBUILDX_ENV_* env(s)
	# shellcheck disable=SC2001
	echo "    export OUT_DIR='$(sed "s%'%%g" <<<"${OUT_DIR:-}" | tr -d '\n')' && $backslash" >>"$dockerfile" # (Only set during compilation.)
	# CARGO_BIN_EXE_<name> — The absolute path to a binary target’s executable. This is only set when building an integration test or benchmark. This may be used with the env macro to find the executable to run for testing purposes. The <name> is the name of the binary target, exactly as-is. For example, CARGO_BIN_EXE_my-program for a binary named my-program. Binaries are automatically built when the test is built, unless the binary has required features that are not enabled.
	# CARGO_PRIMARY_PACKAGE — This environment variable will be set if the package being built is primary. Primary packages are the ones the user selected on the command-line, either with -p flags or the defaults based on the current directory and the default workspace members. This environment variable will not be set when building dependencies. This is only set when compiling the package (not when running binaries or tests).
	# CARGO_TARGET_TMPDIR — Only set when building integration test or benchmark code. This is a path to a directory inside the target directory where integration tests or benchmarks are free to put any data needed by the tests/benches. Cargo initially creates this directory but doesn’t manage its content in any way, this is the responsibility of the test code.

	if [[ "$toolchain_stage" != '' ]]; then
		echo "    export RUSTUP_TOOLCHAIN=\"\$(cat /rustup-toolchain)\" && $backslash" >>"$dockerfile"
	fi

	printf '    if ! rustc' >>"$dockerfile"
	for arg in "${args[@]}"; do
		printf " '%s'" "$arg" >>"$dockerfile"
	done
	printf ' %s >/stdout 2>/stderr; then head /std???; exit 1; fi\n' "$input" >>"$dockerfile"

	if [[ "$incremental" != '' ]]; then
		cat <<EOF >>"$dockerfile"
FROM scratch AS $incremental_stage
COPY --from=$rustc_stage $incremental /
EOF
	fi
	cat <<EOF >>"$dockerfile"
FROM scratch AS $stdio_stage
COPY --from=$rustc_stage /stderr /
COPY --from=$rustc_stage /stdout /
FROM scratch AS $out_stage
COPY --from=$rustc_stage $out_dir/*$extra_filename* /
EOF


	declare -A contexts
	if [[ "$input_mount_name" != '' ]]; then
		contexts["$input_mount_name"]=$input_mount_target
	fi
	if [[ "$cwd" != '' ]]; then
		contexts['cwd']=$cwd
	fi
	if [[ "$crate_out" != '' ]]; then
		contexts["$(crate_out_name "$crate_out")"]=$crate_out
	fi
	contexts['rust']=$RUSTCBUILDX_DOCKER_IMAGE


	# TODO: ask upstream `docker buildx bake` for a "dockerfiles" []string bake setting (that concatanates) or some way to inherit multiple dockerfiles (don't forget inlined ones)
	# TODO: ask upstream `docker buildx` for orderless stages (so we can concat Dockerfiles any which way, and save another DAG)

	declare -A extern_dockerfiles
	printf '# syntax=%s\n' "$RUSTCBUILDX_DOCKER_SYNTAX" >"$dockerfile"~
	for extern_bakefile in "${bakefiles[@]}"; do
		local mounts=0
		while read -r mount_name target; do
			((mounts++)) || true
			mount_name=$(cut -d'"' -f2 <<<"$mount_name")
			target=$(cut -d'"' -f2 <<<"$target")
			contexts["$mount_name"]="$target"
		done < <(grep -E '^\s+"input_|^\s+"crate_out-' "$extern_bakefile")

		local extern_dockerfile=$extern_bakefile
		extern_dockerfile=${extern_dockerfile##*/}
	# extern_dockerfile=_target/debug/strsim-8ed1051e7e58e636.hcl
		extern_dockerfile=${extern_dockerfile##*/}
	# extern_dockerfile=strsim-8ed1051e7e58e636.hcl
		extern_dockerfile=${extern_dockerfile#*-}
	# extern_dockerfile=8ed1051e7e58e636.hcl
		extern_dockerfile=${extern_dockerfile%*.hcl}
	# extern_dockerfile=8ed1051e7e58e636
		extern_dockerfile=$target_path/$extern_dockerfile.Dockerfile
	# extern_dockerfile=_target/debug/strsim-8ed1051e7e58e636.Dockerfile
		extern_dockerfiles["$extern_dockerfile"]=$mounts
	done
	# TODO: concat dockerfiles from topological sort of the DAG (stages must be defined first, then used)
	for (( i_mounts=0; i_mounts<999999; i_mounts++ )); do
		set +u
		local left=${#extern_dockerfiles[@]}
		set -u
		if [[ "$left" = 0 ]]; then
			break
		fi
		for extern_dockerfile in "${!extern_dockerfiles[@]}"; do
			if [[ "$i_mounts" = "${extern_dockerfiles["$extern_dockerfile"]}" ]]; then
				sed 's%^# syntax=.*$%%g' "$extern_dockerfile" >>"$dockerfile"~
				unset 'extern_dockerfiles[$extern_dockerfile]'
			fi
		done
	done
	sed 's%^# syntax=.*$%%g' "$dockerfile" >>"$dockerfile"~


	local bakefile=$target_path/$crate_name$extra_filename.hcl
	local platform=local
	local stdio
	stdio=$(mktemp -d)
	cat <<EOF >"$bakefile"
target "$out_stage" {
	contexts = {
$(for name in "${!contexts[@]}"; do # TODO: sort keys
	printf '\t\t"%s" = "%s",\n' "$name" "${contexts[$name]}"
done)
	}
	dockerfile-inline = <<DOCKERFILE
$(cat "$dockerfile"~)
DOCKERFILE
	network = "none"
	output = ["$out_dir"] # https://github.com/moby/buildkit/issues/1224
	platforms = ["$platform"]
	target = "$out_stage"
}
target "$stdio_stage" {
	inherits = ["$out_stage"]
	output = ["$stdio"]
	target = "$stdio_stage"
}
EOF
	rm "$dockerfile"~; unset dockerfile

	local stages=("$out_stage" "$stdio_stage")
	if [[ "$incremental" != '' ]]; then
		stages+=("$incremental_stage")
		cat <<EOF >>"$bakefile"
target "$incremental_stage" {
	inherits = ["$out_stage"]
	output = ["$incremental"]
	target = "$incremental_stage"
}
EOF
	fi


	err=0
	set +e
	if [[ "${RUSTCBUILDX_DEBUG:-}" == '1' ]]; then
		cat "$bakefile" >&2
		docker --debug buildx bake --file="$bakefile" "${stages[@]}" >&2
	else
		docker         buildx bake --file="$bakefile" "${stages[@]}" >/dev/null 2>&1
	fi
	err=$?
	set -e
	if [[ $err -eq 0 ]]; then
		cat "$stdio/stderr" >&2
		cat "$stdio/stdout"
	fi
	if ! [[ "${RUSTCBUILDX_DEBUG:-}" == '1' ]]; then
		rm "$stdio/stderr" >/dev/null 2>&1 || true
		rm "$stdio/stdout" >/dev/null 2>&1 || true
		rmdir "$stdio" >/dev/null 2>&1 || true
		[[ "$cwd" != '' ]] && rm -r "${cwd:?}/"
	fi
	if [[ "${RUSTCBUILDX_DEBUG:-}" == '1' ]]; then
		rm /tmp/global.lock >/dev/null 2>&1
		return $err
	elif [[ $err -ne 0 ]]; then
		args=()
		for arg in "$@"; do
			if [[ "$arg" =~ ^feature= ]]; then
				arg="feature=\"${arg#feature=}\""
			fi
			args+=("$arg")
		done
		# Bubble up actual error & outputs
		rustc "${args[@]}"
		echo "Found a bug in this script!" 1>&2
		return
	fi
}

if [[ $# -ne 0 ]]; then
	shift # Drop 'rustc' from $@
	if [[ "${1:-}" == '-' ]]; then
		exec "$(which rustc)" "$@"
	else
		_rustc "$@"
		exit
	fi
fi


# Reproduce a working build: (main @ db53336) (docker buildx version: github.com/docker/buildx v0.10.4 c513d34) linux/amd64
# trash _target /tmp/global.lock >/dev/null 2>&1; shellcheck ./tryin.sh && if RUSTCBUILDX_DEBUG=1 CARGO_TARGET_DIR=$PWD/_target ./tryin.sh; then echo YAY; else echo FAILED && tree _target; fi

CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}
PROFILE=${PROFILE:-debug}
mkdir -p "$CARGO_HOME/git/db"
mkdir -p "$CARGO_HOME/git/checkouts"
mkdir -p "$CARGO_HOME/registry/index"
mkdir -p "$CARGO_HOME/registry/cache"
mkdir -p "$CARGO_HOME/registry/src"

ensure() {
	local hash=$1; shift
	local dir=${1:-$(basename "$CARGO_TARGET_DIR")}
	h=$(tar -cf- --directory="$PWD" --sort=name --mtime='UTC 2023-04-15' --group=0 --owner=0 --numeric-owner "$dir" 2>/dev/null | sha256sum)
	[[ "$h" == "$hash  -" ]]
}

export RUSTCBUILDX_DOCKER_IMAGE=docker-image://docker.io/library/rust:1.68.2-slim@sha256:df4d8577fab8b65fabe9e7f792d6f4c57b637dd1c595f3f0a9398a9854e17094 # rustc 1.68.2 (9eb3afe9e 2023-03-27)

toml() {
	local prefix=$1; shift
	grep -F "$prefix" "$PWD"/Cargo.toml | head -n1 | cut -c$((1 + ${#prefix}))-
}
# shellcheck disable=SC2155
export CARGO_PKG_AUTHORS=$(toml 'authors = ')
# shellcheck disable=SC2155
export CARGO_PKG_VERSION=$(toml 'version = ')
# shellcheck disable=SC2155
export CARGO_PKG_DESCRIPTION=$(toml 'description = ')

_rustc --crate-name build_script_build --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/io-lifetimes-1.0.3/build.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type bin --emit=dep-info,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="close"' --cfg 'feature="default"' --cfg 'feature="libc"' --cfg 'feature="windows-sys"' -C metadata=5fc4d6e9dda15f11 -C extra-filename=-5fc4d6e9dda15f11 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/build/io-lifetimes-5fc4d6e9dda15f11 -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 81d7ebfda3a0eb99ea43ecce1f6a626fca4f44acd8c4c56e9736a9ba814aad5c "$CARGO_TARGET_DIR/$PROFILE"/build/io-lifetimes-5fc4d6e9dda15f11
_rustc --crate-name build_script_build "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/libc-0.2.140/build.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type bin --emit=dep-info,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="default"' --cfg 'feature="extra_traits"' --cfg 'feature="std"' -C metadata=beb72f2d4f0e8864 -C extra-filename=-beb72f2d4f0e8864 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/build/libc-beb72f2d4f0e8864 -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure dd885bb5d3321ac15af98193103e92981278728bf2ec0a63e7639947b739fec7 "$CARGO_TARGET_DIR/$PROFILE"/build/libc-beb72f2d4f0e8864
_rustc --crate-name build_script_build --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/rustix-0.37.6/build.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type bin --emit=dep-info,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="default"' --cfg 'feature="fs"' --cfg 'feature="io-lifetimes"' --cfg 'feature="libc"' --cfg 'feature="std"' --cfg 'feature="termios"' --cfg 'feature="use-libc-auxv"' -C metadata=2a01a00f5bdd1924 -C extra-filename=-2a01a00f5bdd1924 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/build/rustix-2a01a00f5bdd1924 -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 6da798a59705347866914b58853dac8e67007449e696e8cdc50303c3fd50d0f0 "$CARGO_TARGET_DIR/$PROFILE"/build/rustix-2a01a00f5bdd1924
_rustc --crate-name bitflags --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/bitflags-1.3.2/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="default"' -C metadata=f255a966af175049 -C extra-filename=-f255a966af175049 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 8663ccfb69487ac1594fb52a3d1083489a7c161f674fba1cbbdde0284288ae71 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name linux_raw_sys --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/linux-raw-sys-0.3.1/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="errno"' --cfg 'feature="general"' --cfg 'feature="ioctl"' --cfg 'feature="no_std"' -C metadata=67b8335e06167307 -C extra-filename=-67b8335e06167307 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure febac1d2841ecb1a329f56925ac841aed0e31ccf2a47e5141c13c053f090eb61 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name build_script_build --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/proc-macro2-1.0.56/build.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type bin --emit=dep-info,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="default"' --cfg 'feature="proc-macro"' -C metadata=349a49cf19c07c83 -C extra-filename=-349a49cf19c07c83 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/build/proc-macro2-349a49cf19c07c83 -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 4a6b8d975ad57077ab74f1acc84b70b045373dc84187536a691d6ab7a331e5a8 "$CARGO_TARGET_DIR/$PROFILE"/build/proc-macro2-349a49cf19c07c83
_rustc --crate-name unicode_ident --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/unicode-ident-1.0.5/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 -C metadata=417636671c982ef8 -C extra-filename=-417636671c982ef8 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 63debee9e52e3f92c21060c264b7cfa277623245d4312595b76a05ae7de9fe18 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name build_script_build --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/quote-1.0.26/build.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type bin --emit=dep-info,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="default"' --cfg 'feature="proc-macro"' -C metadata=de6232726d2cb6c6 -C extra-filename=-de6232726d2cb6c6 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/build/quote-de6232726d2cb6c6 -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 5286cc8117c408ddfa66b081db97549c938bc29090b30f7da7f94ffba5a8c270 "$CARGO_TARGET_DIR/$PROFILE"/build/quote-de6232726d2cb6c6
_rustc --crate-name utf8parse --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/utf8parse-0.2.1/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="default"' -C metadata=951ca9bdc6d60a50 -C extra-filename=-951ca9bdc6d60a50 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 3a9d88dcc4808ec4a6613fca714f98cc0cefc96a4b81ed9007e7e6f3ab19e446 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name anstyle --edition=2021 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/anstyle-0.3.5/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="default"' --cfg 'feature="std"' -C metadata=3d9b242388653423 -C extra-filename=-3d9b242388653423 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 515ebdbaea4523d75db0f98b38fa07c353834c6af3fdfdd1fce1f76157dccb41 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name anstyle_parse --edition=2021 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/anstyle-parse-0.1.1/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="default"' --cfg 'feature="utf8"' -C metadata=0d4af9095c79189b -C extra-filename=-0d4af9095c79189b --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern utf8parse="$CARGO_TARGET_DIR/$PROFILE"/deps/libutf8parse-951ca9bdc6d60a50.rmeta --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 59c82c7d16960ea23be7db27c69d632a54a1faa9def5f7bf4b9e631f5e8b5025 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name concolor_override --edition=2021 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/concolor-override-1.0.0/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 -C metadata=305fddcda33650f6 -C extra-filename=-305fddcda33650f6 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 242e5eefd61963ac190559f7061228a18a5e1820f75062354d6c3aac7adb9c8d "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name concolor_query --edition=2021 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/concolor-query-0.3.3/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 -C metadata=74e38d373bc944a9 -C extra-filename=-74e38d373bc944a9 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 80b4cf0c70fba99090291a5a9f18996b89d7c48d34fe47a7b7654260f9061d93 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name strsim "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/strsim-0.10.0/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 -C metadata=8ed1051e7e58e636 -C extra-filename=-8ed1051e7e58e636 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 7d0ab51edc54d1fba6562eba15c2d50b006d8b1d26c7caaa4835ca842b32e6bd "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name clap_lex --edition=2021 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/clap_lex-0.4.1/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 -C metadata=7dfc2f58447e727e -C extra-filename=-7dfc2f58447e727e --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 102ebf5186bfb97efed2644993b22c8eba60f758a24f8ca0795da74196afb3d5 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name heck --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/heck-0.4.0/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="default"' -C metadata=cd1cdbedec0a6dc0 -C extra-filename=-cd1cdbedec0a6dc0 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure fc9fd4659e849560714e36514cfc6024d3ba3bf9356806479aaeb46417f24b06 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name proc_macro2 --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/proc-macro2-1.0.56/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="default"' --cfg 'feature="proc-macro"' -C metadata=ef119f7eb3ef5720 -C extra-filename=-ef119f7eb3ef5720 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern unicode_ident="$CARGO_TARGET_DIR/$PROFILE"/deps/libunicode_ident-417636671c982ef8.rmeta --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold --cfg use_proc_macro --cfg wrap_proc_macro
ensure b7382060b42396c1cef74ce8573fffb7bc13cb4ad97d07c0aa3e6367275135a7 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name libc "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/libc-0.2.140/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="default"' --cfg 'feature="extra_traits"' --cfg 'feature="std"' -C metadata=9de7ca31dbbda4df -C extra-filename=-9de7ca31dbbda4df --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold --cfg freebsd11 --cfg libc_priv_mod_use --cfg libc_union --cfg libc_const_size_of --cfg libc_align --cfg libc_int128 --cfg libc_core_cvoid --cfg libc_packedN --cfg libc_cfg_target_vendor --cfg libc_non_exhaustive --cfg libc_long_array --cfg libc_ptr_addr_of --cfg libc_underscore_const_names --cfg libc_const_extern_fn
ensure aca31b437a30b41437831eb55833d4a603cd17b1bf14bf61c22fd597cc85f591 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name once_cell --edition=2021 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/once_cell-1.15.0/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="alloc"' --cfg 'feature="default"' --cfg 'feature="race"' --cfg 'feature="std"' -C metadata=da1c67e98ff0d3df -C extra-filename=-da1c67e98ff0d3df --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 681724646da3eeff1fc4eac763657069367f2eca3063541d6210267af9516c8b "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name fastrand --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/fastrand-1.8.0/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 -C metadata=f39af6f065361be9 -C extra-filename=-f39af6f065361be9 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure a96b56af7d56ecc76ba33d69c403e79d35ad219f4cf33534c7de79dcd869a418 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name shlex "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/shlex-1.1.0/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="default"' --cfg 'feature="std"' -C metadata=df9eb4fba8dd532e -C extra-filename=-df9eb4fba8dd532e --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 8e7d662c23280112c77833c899c60295549f40cef9c30d44fa3ee5f2cae9c70c "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name cfg_if --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/cfg-if-1.0.0/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 -C metadata=305ff6ac5e1cfc5a -C extra-filename=-305ff6ac5e1cfc5a --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 5b338fbcc1fa1566b2700d106204b426c2914d8ae9dd73a6caa8b54600940acb "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name quote --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/quote-1.0.26/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="default"' --cfg 'feature="proc-macro"' -C metadata=74434efe692a445d -C extra-filename=-74434efe692a445d --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern proc_macro2="$CARGO_TARGET_DIR/$PROFILE"/deps/libproc_macro2-ef119f7eb3ef5720.rmeta --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure ddb97c0389c55493bea292d9853dcada9d9f4207dbe7bad52d217b086e3706bc "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name syn --edition=2021 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/syn-2.0.13/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="clone-impls"' --cfg 'feature="default"' --cfg 'feature="derive"' --cfg 'feature="full"' --cfg 'feature="parsing"' --cfg 'feature="printing"' --cfg 'feature="proc-macro"' --cfg 'feature="quote"' -C metadata=4befa7538c9a9f80 -C extra-filename=-4befa7538c9a9f80 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern proc_macro2="$CARGO_TARGET_DIR/$PROFILE"/deps/libproc_macro2-ef119f7eb3ef5720.rmeta --extern quote="$CARGO_TARGET_DIR/$PROFILE"/deps/libquote-74434efe692a445d.rmeta --extern unicode_ident="$CARGO_TARGET_DIR/$PROFILE"/deps/libunicode_ident-417636671c982ef8.rmeta --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 935c4792b4e7e86fae13917aebbdccbf3a05c26856f5b09dcfa5386bd52bde6e "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name io_lifetimes --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/io-lifetimes-1.0.3/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="close"' --cfg 'feature="default"' --cfg 'feature="libc"' --cfg 'feature="windows-sys"' -C metadata=36f41602071771e6 -C extra-filename=-36f41602071771e6 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern libc="$CARGO_TARGET_DIR/$PROFILE"/deps/liblibc-9de7ca31dbbda4df.rmeta --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold --cfg io_safety_is_in_std --cfg panic_in_const_fn
ensure 112d5c4bc459c38843f5ce37369ce675069018518aa5fa458664e1b535ed21e5 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name rustix --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/rustix-0.37.6/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="default"' --cfg 'feature="fs"' --cfg 'feature="io-lifetimes"' --cfg 'feature="libc"' --cfg 'feature="std"' --cfg 'feature="termios"' --cfg 'feature="use-libc-auxv"' -C metadata=120609be99d53c6b -C extra-filename=-120609be99d53c6b --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern bitflags="$CARGO_TARGET_DIR/$PROFILE"/deps/libbitflags-f255a966af175049.rmeta --extern io_lifetimes="$CARGO_TARGET_DIR/$PROFILE"/deps/libio_lifetimes-36f41602071771e6.rmeta --extern libc="$CARGO_TARGET_DIR/$PROFILE"/deps/liblibc-9de7ca31dbbda4df.rmeta --extern linux_raw_sys="$CARGO_TARGET_DIR/$PROFILE"/deps/liblinux_raw_sys-67b8335e06167307.rmeta --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold --cfg linux_raw --cfg asm --cfg linux_like
ensure 9bb7a1fc562b237a662b413a3d21e79bce55a868b4349ff12539c6820bbf3824 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name tempfile --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/tempfile-3.5.0/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 -C metadata=018ce729f986d26d -C extra-filename=-018ce729f986d26d --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern cfg_if="$CARGO_TARGET_DIR/$PROFILE"/deps/libcfg_if-305ff6ac5e1cfc5a.rmeta --extern fastrand="$CARGO_TARGET_DIR/$PROFILE"/deps/libfastrand-f39af6f065361be9.rmeta --extern rustix="$CARGO_TARGET_DIR/$PROFILE"/deps/librustix-120609be99d53c6b.rmeta --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 7e07d230803dec87e97701ec18626446659c84cd451986d727c83844520fc7f8 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name is_terminal --edition=2018 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/is-terminal-0.4.7/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 -C metadata=4b94fef286899229 -C extra-filename=-4b94fef286899229 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern io_lifetimes="$CARGO_TARGET_DIR/$PROFILE"/deps/libio_lifetimes-36f41602071771e6.rmeta --extern rustix="$CARGO_TARGET_DIR/$PROFILE"/deps/librustix-120609be99d53c6b.rmeta --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 15bdbb8397e2cbcd7a1df2589fc4f1bc1c53dbe0bee28d5bd86a99e003ce72b5 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name anstream --edition=2021 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/anstream-0.2.6/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="auto"' --cfg 'feature="default"' --cfg 'feature="wincon"' -C metadata=47e0535dab3ef0d2 -C extra-filename=-47e0535dab3ef0d2 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern anstyle="$CARGO_TARGET_DIR/$PROFILE"/deps/libanstyle-3d9b242388653423.rmeta --extern anstyle_parse="$CARGO_TARGET_DIR/$PROFILE"/deps/libanstyle_parse-0d4af9095c79189b.rmeta --extern concolor_override="$CARGO_TARGET_DIR/$PROFILE"/deps/libconcolor_override-305fddcda33650f6.rmeta --extern concolor_query="$CARGO_TARGET_DIR/$PROFILE"/deps/libconcolor_query-74e38d373bc944a9.rmeta --extern is_terminal="$CARGO_TARGET_DIR/$PROFILE"/deps/libis_terminal-4b94fef286899229.rmeta --extern utf8parse="$CARGO_TARGET_DIR/$PROFILE"/deps/libutf8parse-951ca9bdc6d60a50.rmeta --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure fe77f7f8efbf78a44e9e2dc25b2b2af3823a48cb22b200c29300ddc38a701abc "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name clap_builder --edition=2021 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/clap_builder-4.2.1/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="color"' --cfg 'feature="error-context"' --cfg 'feature="help"' --cfg 'feature="std"' --cfg 'feature="suggestions"' --cfg 'feature="usage"' -C metadata=02591a0046469edd -C extra-filename=-02591a0046469edd --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern anstream="$CARGO_TARGET_DIR/$PROFILE"/deps/libanstream-47e0535dab3ef0d2.rmeta --extern anstyle="$CARGO_TARGET_DIR/$PROFILE"/deps/libanstyle-3d9b242388653423.rmeta --extern bitflags="$CARGO_TARGET_DIR/$PROFILE"/deps/libbitflags-f255a966af175049.rmeta --extern clap_lex="$CARGO_TARGET_DIR/$PROFILE"/deps/libclap_lex-7dfc2f58447e727e.rmeta --extern strsim="$CARGO_TARGET_DIR/$PROFILE"/deps/libstrsim-8ed1051e7e58e636.rmeta --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure abc761d7e43bb11db79e0823f549a1942df01d89bfa782b26fe06073c8948156 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name clap_derive --edition=2021 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/clap_derive-4.2.0/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type proc-macro --emit=dep-info,link -C prefer-dynamic -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="default"' -C metadata=a4ff03e749cd3808 -C extra-filename=-a4ff03e749cd3808 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern heck="$CARGO_TARGET_DIR/$PROFILE"/deps/libheck-cd1cdbedec0a6dc0.rlib --extern proc_macro2="$CARGO_TARGET_DIR/$PROFILE"/deps/libproc_macro2-ef119f7eb3ef5720.rlib --extern quote="$CARGO_TARGET_DIR/$PROFILE"/deps/libquote-74434efe692a445d.rlib --extern syn="$CARGO_TARGET_DIR/$PROFILE"/deps/libsyn-4befa7538c9a9f80.rlib --extern proc_macro --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure f5e51647aa7f63210277423b50332fe3c3e0a018e10554a472b13402591b4ec1 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name clap --edition=2021 "$CARGO_HOME"/registry/src/github.com-1ecc6299db9ec823/clap-4.2.1/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 --cfg 'feature="color"' --cfg 'feature="default"' --cfg 'feature="derive"' --cfg 'feature="error-context"' --cfg 'feature="help"' --cfg 'feature="std"' --cfg 'feature="suggestions"' --cfg 'feature="usage"' -C metadata=8996e440435cdc93 -C extra-filename=-8996e440435cdc93 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern clap_builder="$CARGO_TARGET_DIR/$PROFILE"/deps/libclap_builder-02591a0046469edd.rmeta --extern clap_derive="$CARGO_TARGET_DIR/$PROFILE"/deps/libclap_derive-a4ff03e749cd3808.so --extern once_cell="$CARGO_TARGET_DIR/$PROFILE"/deps/libonce_cell-da1c67e98ff0d3df.rmeta --cap-lints allow -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure ce6e368345e52f1d35c56552efac510ef2c7610738e3af297b54098db62ab537 "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name buildxargs --edition=2021 src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type lib --emit=dep-info,metadata,link -C embed-bitcode=no -C debuginfo=2 -C metadata=1052b4790952332f -C extra-filename=-1052b4790952332f --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -C incremental="$CARGO_TARGET_DIR/$PROFILE"/incremental -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern clap="$CARGO_TARGET_DIR/$PROFILE"/deps/libclap-8996e440435cdc93.rmeta --extern shlex="$CARGO_TARGET_DIR/$PROFILE"/deps/libshlex-df9eb4fba8dd532e.rmeta --extern tempfile="$CARGO_TARGET_DIR/$PROFILE"/deps/libtempfile-018ce729f986d26d.rmeta -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure c68b91d305505ea79d63b8243c0a23d4356fe01e7fb8514d4b5b1b283d9f57ba "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name buildxargs --edition=2021 src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --emit=dep-info,link -C embed-bitcode=no -C debuginfo=2 --test -C metadata=4248d2626f765b01 -C extra-filename=-4248d2626f765b01 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -C incremental="$CARGO_TARGET_DIR/$PROFILE"/incremental -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern clap="$CARGO_TARGET_DIR/$PROFILE"/deps/libclap-8996e440435cdc93.rlib --extern shlex="$CARGO_TARGET_DIR/$PROFILE"/deps/libshlex-df9eb4fba8dd532e.rlib --extern tempfile="$CARGO_TARGET_DIR/$PROFILE"/deps/libtempfile-018ce729f986d26d.rlib -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure ae4a8f2c5cf8b25feb3b697231a835ab87dbd64c36306e84b539ded52ccd66ab "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name buildxargs --edition=2021 src/main.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --emit=dep-info,link -C embed-bitcode=no -C debuginfo=2 --test -C metadata=9b4fb3065c88e032 -C extra-filename=-9b4fb3065c88e032 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -C incremental="$CARGO_TARGET_DIR/$PROFILE"/incremental -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern buildxargs="$CARGO_TARGET_DIR/$PROFILE"/deps/libbuildxargs-1052b4790952332f.rlib --extern clap="$CARGO_TARGET_DIR/$PROFILE"/deps/libclap-8996e440435cdc93.rlib --extern shlex="$CARGO_TARGET_DIR/$PROFILE"/deps/libshlex-df9eb4fba8dd532e.rlib --extern tempfile="$CARGO_TARGET_DIR/$PROFILE"/deps/libtempfile-018ce729f986d26d.rlib -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 035362f78c3f7776f743cd00c511f22a16a1ef126c2941fed16a777e534d34ec "$CARGO_TARGET_DIR/$PROFILE"/deps
_rustc --crate-name buildxargs --edition=2021 src/main.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat --diagnostic-width=211 --crate-type bin --emit=dep-info,link -C embed-bitcode=no -C debuginfo=2 -C metadata=357a2a97fcd61762 -C extra-filename=-357a2a97fcd61762 --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps -C linker=/usr/bin/clang -C incremental="$CARGO_TARGET_DIR/$PROFILE"/incremental -L dependency="$CARGO_TARGET_DIR/$PROFILE"/deps --extern buildxargs="$CARGO_TARGET_DIR/$PROFILE"/deps/libbuildxargs-1052b4790952332f.rlib --extern clap="$CARGO_TARGET_DIR/$PROFILE"/deps/libclap-8996e440435cdc93.rlib --extern shlex="$CARGO_TARGET_DIR/$PROFILE"/deps/libshlex-df9eb4fba8dd532e.rlib --extern tempfile="$CARGO_TARGET_DIR/$PROFILE"/deps/libtempfile-018ce729f986d26d.rlib -C link-arg=-fuse-ld=/usr/local/bin/mold
ensure 08b87ec01aa295e78efadc2cc7a2dfa4a96ff2c54cab32cc8b71dd2ef18a253b "$CARGO_TARGET_DIR/$PROFILE"/deps
