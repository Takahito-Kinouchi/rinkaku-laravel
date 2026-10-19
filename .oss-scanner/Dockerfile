# Scanner image for https://github.com/Takahito-Kinouchi/rinkaku-laravel (OSS Scanner enrolment).
#
# The audit runs offline, so this build installs the pinned toolchain, fetches every crate, and compiles the
# workspace, its tests and the release binary. threat_model.md, next to this file, says how to exercise them.
FROM rust:1-bookworm
ENV DEBIAN_FRONTEND=noninteractive
# git is what rinkaku shells out to for `--base` and file reads; the image already has it and a C toolchain for the
# tree-sitter grammars.
RUN sed -i 's|http://deb.debian.org|https://deb.debian.org|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update && apt-get install -y --no-install-recommends git python3 \
    && rm -rf /var/lib/apt/lists/*

# scanner contract: the checkout lives inside the image, at /src
COPY . /src
WORKDIR /src

# rust-toolchain.toml pins the channel; install it now, while the network is up.
RUN rustup show active-toolchain || rustup toolchain install

# Tests and every `--base` run commit to throwaway repositories, which needs a git identity.
RUN git config --global user.name scanner && git config --global user.email scanner@localhost \
    && git config --global --add safe.directory '*'

# An ARG, so that it bounds the memory this build uses without reaching the audit environment.
ARG CARGO_BUILD_JOBS=8

# A step that fails fails the build, and primary_contact gets the log.
RUN cargo fetch \
    && cargo build --workspace --all-targets --all-features \
    && cargo test --workspace --all-features --no-run \
    && cargo build --release -p rinkaku-laravel

# Run the tests so the log shows the image works, without letting a failure abort the build.
RUN timeout 1800 cargo test --workspace --all-features || echo "WARNING: cargo test reported failures"

# Everything is fetched. From here on cargo must not wait for a registry that it cannot reach.
ENV CARGO_NET_OFFLINE=true
