# PKGBUILD For lkstrap and lkfstab

# Contributor: Janorovic Volkov <janorovicvolkov@gmail.com>
# Maintainer: Janorovic Volkov <janorovicvolkov@gmail.com>

pkgname=liska-install-scripts
pkgver=1
pkgrel=1
pkgdesc="Liska Linux Install Helper Scripts"
arch=('x86_64')
url="https://github.com/source-liskalinux/liska-install-scripts"
license=('GPL-3.0-or-later')
depends=('lkpm' 'ca-certificates')
makedepends=('rust')

build() {
    echo "--> [BUILD] Compiling lkstrap and lkfstab...."
    cargo build --release
}

package() {
    install -d "${pkgdir}/usr/bin"
    echo "--> [PACKAGE] Installing lkstrap...."
    install -Dm755 "./target/release/lkstrap" "${pkgdir}/usr/bin/lkstrap"
    echo "--> [PACKAGE] Installing lkfstab...."
    install -Dm755 "./target/release/lkfstab" "${pkgdir}/usr/bin/lkfstab"
}
