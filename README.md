# confsync

Linux yapılandırma dosyalarını git deposuna yedekleyen ve geri yükleyen
masaüstü uygulaması. Rust + egui.

Tasarım kararları ve gerekçeleri için [DESIGN.md](DESIGN.md).

## Gereksinimler

```bash
# Rust 1.81+
rustup update stable

# Derleme bağımlılıkları (Debian/Ubuntu)
sudo apt install build-essential pkg-config libssl-dev cmake

# Fedora
sudo dnf install gcc-c++ pkgconf-pkg-config openssl-devel cmake
```

## Derleme ve çalıştırma

```bash
cargo run -p confsync-gui --release
```

## Arch Linux paketi

```bash
cd packaging
makepkg -f                                   # paketi üretir (testleri de çalıştırır)
sudo pacman -U confsync-0.1.0-1-x86_64.pkg.tar.zst
```

Paket şunları kurar:

| | |
|---|---|
| `/usr/bin/confsync` | masaüstü arayüzü |
| `/usr/bin/confsync-agent` | tray ajanı |
| `/usr/share/applications/confsync.desktop` | uygulama menüsü girdisi |
| `/usr/lib/systemd/user/confsync-agent.service` | kullanıcı servisi |
| `…/graphical-session.target.wants/confsync-agent.service` | etkinleştirme bağlantısı |

Son satır sayesinde ajan **paketle birlikte etkin gelir**: `systemctl --user
enable` çalıştırmak gerekmez, grafik oturum açıldığında kendiliğinden başlar.
Kurulumdan hemen sonra, oturumu yeniden açmadan başlatmak için:

```bash
systemctl --user start confsync-agent.service
```

Ajanı istemiyorsanız `disable` yetmez (etkinleştirme paketten gelir):

```bash
systemctl --user mask confsync-agent.service
```

Kaldırmak için `sudo pacman -R confsync`. Ayarlar ve yedek deposu kalır.

Testler (GUI gerekmez):

```bash
cargo test -p confsync-core
```

## Kullanım

1. **Ayarlar** sekmesinde yerel depo yolunu ve varsa uzak depo adresini girin.
   Uzak depo için ssh-agent'ınızdaki anahtar ya da git credential helper
   kullanılır; uygulama parola saklamaz.
2. **Kaynaklar** sekmesinde yedeklenecek klasör ve dosyaları seçin. `~/.config`
   bütün olarak eklenmez (içinde tarayıcı profilleri ve uygulama durumu vardır);
   bilinen yapılandırma girdileri tek tek gelir, gerisi için keşif panelini
   kullanın.
3. **Hariç Tutulanlar** sekmesinde gitignore sözdizimiyle kalıp yazın.
   Önbellek dizinleri ve bilinen anahtar dosyaları varsayılan listede zaten var.
4. **Şimdi Yedekle** düğmesi tarar, depoya kopyalar ve commit atar.
   Hiçbir şey değişmediyse boş commit atılmaz.
5. **Geri Yükle** sekmesinde önce plan çıkarılır — bu adımda diske hiçbir şey
   yazılmaz. Listeyi gözden geçirip onayladığınızda dosyalar yazılır; üzerine
   yazılan her dosyanın kopyası `~/.local/share/confsync/rollback/` altına alınır.

## Dosya konumları

| Yol | İçerik |
|---|---|
| `~/.config/confsync/settings.toml` | Uygulama ayarları |
| `~/.local/share/confsync/repo/` | Varsayılan yerel git deposu |
| `~/.local/share/confsync/rollback/` | Geri yükleme öncesi güvenlik kopyaları |

## Uyarı

Sır tespiti sezgiseldir ve kesin değildir. Uzak depoyu herkese açık yapmadan
önce **Genel Bakış** sekmesindeki "Atlanan dosyalar" listesini ve deponun
içeriğini gözden geçirin.

## Ajan (tray)

`confsync-agent` penceresiz çalışır: trayde bir ikon olarak durur, kaynakları
düzenli aralıklarla denetler ve değişiklik bulunca bildirim gönderir.

```bash
cargo run -p confsync-agent --release      # trayde çalıştır
confsync-agent --once                      # tek denetim, ekrana yaz, çık
confsync-agent --backup                    # tek yedekleme, çık
```

İkon rengi durumu gösterir: yeşil (her şey yedeklendi), mavi (bekleyen
değişiklik), turuncu (kararınız gerekiyor), kırmızı (hata), gri (duraklatıldı).
Sol tık arayüzü açar; sağ tık menüsünde denetleme, yedekleme ve duraklatma var.

Aralık (varsayılan 5 dakika) ve otomatik yedekleme, arayüzdeki **Ayarlar →
Ajan** bölümünden yönetilir; ajan bunları her turda yeniden okur.

Karar gerektiren dosya (sır şüphesi, boyut sınırı) varsa ajan **kendiliğinden
yedeklemez**; yalnızca bildirim gönderir ve kararı arayüzdeki onay penceresine
bırakır.

Oturum açılışında başlatmak için (paket kuruluysa birim dosyası hazır gelir):

```bash
systemctl --user enable --now confsync-agent.service
```

### Neden anlık (inotify) izleme yok?

Tam tarama tipik bir yapılandırma ağacı için saniyenin altında sürüyor
(ölçüm: 648 dosya / 24 MiB için ~0.4 sn). Düzenli yoklama aynı sonucu, watch
yönetimi ve editörlerin `rename` ile yazma davranışıyla uğraşmadan veriyor.
Gecikme gerçekten sorun olursa aynı ajana tetikleyici olarak eklenebilir.
