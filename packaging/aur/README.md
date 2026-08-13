# AUR paketi

Bu dizin AUR deposuna gidecek dosyaları tutar: `PKGBUILD`, `.SRCINFO` ve
`confsync.install`. AUR deposunda **yalnızca bu üç dosya** bulunur; kaynak
kodu `source=` ile depodan çekilir.

`../PKGBUILD` ise yerel geliştirme içindir: kaynağı indirmez, çalışma
ağacındaki `target/release` çıktısını yeniden kullanır.

## Kaynak deposu

Paket kaynağı GitHub'dan, sürüm etiketiyle çekilir:

```
source=("git+https://github.com/alikaya/confsync.git#tag=v$pkgver")
```

Depo herkese açık; anonim klonlandığını doğrulamak için:

```bash
git clone https://github.com/alikaya/confsync.git /tmp/anon-test
```

Kimlik sormadan klonlanıyorsa AUR kullanıcıları da derleyebilir.

## Gönderim

AUR hesabı ve hesaba tanımlı bir SSH anahtarı gerekir
(https://aur.archlinux.org → My Account → SSH Public Key).

```bash
git clone ssh://aur@aur.archlinux.org/confsync.git /tmp/aur-confsync
cd /tmp/aur-confsync
cp /path/to/confsync/packaging/aur/{PKGBUILD,.SRCINFO,confsync.install} .
git add PKGBUILD .SRCINFO confsync.install
git commit -m "confsync 0.1.0-1: ilk sürüm"
git push
```

`confsync` adı AUR'da boş (kontrol edildi).

## Sürüm çıkarken

1. `Cargo.toml` içindeki `version` alanını yükseltin.
2. Depoda etiketleyin: `git tag -a v0.2.0 -m "confsync 0.2.0" && git push --tags`
3. Bu dizindeki `PKGBUILD` içinde `pkgver`'i güncelleyin, `pkgrel=1` yapın.
4. `.SRCINFO`'yu yeniden üretin — AUR bunu okur, güncellenmezse sürüm
   görünmez:

   ```bash
   makepkg --printsrcinfo > .SRCINFO
   ```

5. Yerel doğrulama (temiz derleme, testler dahil):

   ```bash
   makepkg -f && namcap ./confsync-*.pkg.tar.zst
   ```

6. Üç dosyayı AUR deposuna kopyalayıp push edin.

## Bilinen namcap uyarıları

`libglvnd`, `wayland`, `libxkbcommon*`, `libx11`, `libxcb`, `libxcursor`,
`libxi` için "Dependency included, but may not be needed" uyarısı verir.
Uyarı yanlıştır: bu kütüphaneler `winit`/`glutin` tarafından çalışma anında
`dlopen` ile yüklenir, dolayısıyla ELF `DT_NEEDED` listesinde görünmezler.
İkili içindeki soname dizgileriyle doğrulanmıştır.
