# This is part of WHY2
# Copyright (C) 2022-2026 Václav Šmejkal

# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.

# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.

# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.

FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app
COPY . .

ENV WHY2_CONFIG_DIR="/data"
RUN cargo build --bin why2-server --no-default-features --features server,windows_resources --release

FROM alpine AS runtime

RUN apk add --no-cache libgcc

RUN addgroup -S why2 && adduser -S why2 -G why2

COPY --from=builder /app/target/release/why2-server /usr/local/bin/why2-server

RUN mkdir -p /data && chown -R why2:why2 /data

USER why2

VOLUME /data

EXPOSE 1204/tcp
EXPOSE 1204/udp

CMD ["why2-server"]
