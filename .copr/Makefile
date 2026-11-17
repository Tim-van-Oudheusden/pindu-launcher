# COPR's make_srpm method (#382): makes the source RPM from build-aux/rpm/pindu-launcher.spec
# and the release tarball the spec's Source0 names (pindu-launcher-X.Y.Z.tar.xz, crates
# vendored), checked against the release's SHA256SUMS. COPR runs this as root in a Fedora
# chroot with network:
#
#   make -f .copr/Makefile srpm outdir=OUTDIR [spec=SPEC]
#
# The version is the spec's, so build after the vX.Y.Z release is published (docs/releasing.md).

top := $(abspath $(dir $(lastword $(MAKEFILE_LIST)))/..)
spec ?= $(top)/build-aux/rpm/pindu-launcher.spec
outdir ?= $(CURDIR)
sources := $(outdir)/sources

srpm:
	rpm -q rpm-build rpmdevtools >/dev/null || dnf -y install rpm-build rpmdevtools
	mkdir -p "$(sources)"
	spectool --get-files --directory "$(sources)" "$(spec)"
	cd "$(sources)" && \
	  curl -fsSL -o SHA256SUMS \
	    "$$(rpmspec -q --srpm --qf '%{url}/releases/download/v%{version}/SHA256SUMS' "$(spec)")" && \
	  sha256sum --check --ignore-missing --strict SHA256SUMS | grep -q ': OK$$'
	rpmbuild -bs \
	  --define "_sourcedir $(sources)" \
	  --define "_srcrpmdir $(outdir)" \
	  "$(spec)"

.PHONY: srpm
