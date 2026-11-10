#!/bin/sh
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
work=${CAMOTICS_WORK:-$(mktemp -d)}
cd "$here"
cargo build --release --locked --bin cambench
rm -rf "$work/out"
./target/release/cambench camotics examples/plate.gcam "$work/out"
mkdir -p "$work/pkg"
curl -fsSL -o "$work/pkg/camotics.deb" https://camotics.org/builds/release/debian-stable-64bit/camotics_1.2.0_amd64.deb
docker run --rm -v "$work/pkg:/pkg" debian:stretch sh -c '
  echo "deb http://archive.debian.org/debian stretch main" > /etc/apt/sources.list
  apt-get -o Acquire::Check-Valid-Until=false update -qq >/dev/null 2>&1
  cd /pkg && apt-get download libv8-3.14.5 >/dev/null 2>&1; ls'
docker run --rm -v "$work:/w" debian:buster sh -c '
  sed -i "s/deb.debian.org/archive.debian.org/;s|security.debian.org|archive.debian.org|" /etc/apt/sources.list
  apt-get update -qq >/dev/null 2>&1
  apt-get install -y -qq libqt5websockets5 /w/pkg/libv8*.deb /w/pkg/camotics.deb >/dev/null
  cd /w/out && camsim --binary job.camotics job.stl'
./target/release/cambench agree examples/plate.gcam "$work/out/job.stl"
