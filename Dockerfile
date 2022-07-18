########################################
#        Base Build Environment        #
########################################

FROM rustlang/rust:nightly-bullseye as rust-base

RUN apt update \
    && apt upgrade -yq \
    && apt dist-upgrade -yq \
    && apt install -yq build-essential; \
    apt install -yq gcc-aarch64-linux-gnu || apt install -yq gcc-x86-64-linux-gnu; \
    apt purge -yq \
    && apt-get clean -yq \
    && apt-get autoclean -yq \
    && rm -rf /var/cache/apt/archives/* \
    && rustup toolchain install nightly \
    && rustup default nightly \
    && rustup target add aarch64-unknown-linux-gnu \
                         x86_64-unknown-linux-gnu \
                         aarch64-unknown-linux-musl \
                         x86_64-unknown-linux-musl \
    && rustup component add rust-src \
                            rust-std-aarch64-unknown-linux-gnu \
                            rust-std-aarch64-unknown-linux-musl \
                            rust-std-x86_64-unknown-linux-gnu \
                            rust-std-x86_64-unknown-linux-musl \
    && cargo install cargo-chef


########################################
#          Cache Pre-Planner           #
########################################

FROM rust-base as planner

WORKDIR /artifacts

COPY . /artifacts

# Create a "plan" for the dependency cache
RUN cargo chef prepare --recipe-path recipe.json


########################################
#           Artifact Factory           #
########################################

FROM planner as artifactory

ARG cargo_profile=release

WORKDIR /artifacts

# Copy over the previously created dependency cache plan
COPY --from=planner /artifacts/recipe.json recipe.json

# Execute the plan and cache the project's dependencies
RUN cargo chef cook --recipe-path recipe.json --profile ${cargo_profile}

# Copy over the project's "actual" source ...
COPY . .

# ... and compile it
RUN cargo build --profile ${cargo_profile} \
    && mv "$(pwd)/target/${cargo_profile}/pinnothera" "$(pwd)/pinnothera-bin" \
    && chmod +x "$(pwd)/pinnothera-bin"


#####################################
#           Image Factory           #
#####################################

# Use distroless image for production
FROM gcr.io/distroless/cc as pinn-image

COPY --from=precompiled-artifactory /artifacts/pinnothera-bin /usr/bin/pinnothera

CMD ["/usr/bin/pinnothera"]


########################################
#         Precompiled Factory          #
########################################

FROM alpine:latest as precompiled-artifactory

ARG profile='release'

ENV PINN_PROFILE=${profile}

WORKDIR /artifacts

# Copy over the precompiled binaries
COPY ./artifacts/ /artifacts/precompiled

RUN export TARGET_OS="$(uname -s | awk '{ print tolower($0) }')" \
    && export TARGET_ARCH="$(uname -m | sed 's#x86_64#amd64#g; s#aarch64#arm64#g')" \
    && mv "$(pwd)/precompiled/pinnothera-${TARGET_ARCH}-${TARGET_OS}-${PINN_PROFILE}" "$(pwd)/pinnothera-bin" \
    && rm -rf "$(pwd)/precompiled" \
    && chmod +x "$(pwd)/pinnothera-bin"



#####################################
#           Image Factory           #
#####################################

# Use distroless image for production
FROM gcr.io/distroless/cc as precompiled-pinn-image

COPY --from=precompiled-artifactory /artifacts/pinnothera-bin /usr/bin/pinnothera

CMD ["/usr/bin/pinnothera"]
