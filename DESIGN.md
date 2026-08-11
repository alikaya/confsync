# confsync — Tasarım Notları

Linux yapılandırma dosyalarını bir git deposuna yedekleyen ve geri yükleyen
masaüstü uygulaması. Bu belge, kod yazılmadan önce verilmesi gereken kararları
ve bunların gerekçelerini toplar.

---

## 1. Teknoloji seçimleri

| Alan | Seçim | Gerekçe |
|---|---|---|
| GUI | **egui / eframe** | Tek statik ikili dosya üretir, GTK/Qt runtime bağımlılığı yok. Dağıtımlar arası taşınabilirlik bu uygulama için kritik: aracın kendisi kurulum gerektirirse, "yeni makineyi ayağa kaldırma" senaryosunda işe yaramaz. |
| Git | **git2 (libgit2)** | `git` CLI'ı `Command` ile çağırmak yerine kütüphane kullanmak, hata yönetimini ve ilerleme bildirimini düzgün yapmayı sağlar. Kimlik doğrulama ssh-agent ve credential helper üzerinden devralınır. |
| Hariç tutma | **`ignore` crate (gitignore sözdizimi)** | Kullanıcı zaten bildiği bir dili kullanır; `!` ile istisna tanımlayabilir. Kendi kalıp dilimizi icat etmek gereksiz. |
| Ayar biçimi | **TOML** | Elle düzenlenebilir, yorum destekler. |
| Manifest | **JSON** | Makine üretimi; satır bazlı diff'i git'te okunaklı. |

### Neden alternatifler değil

- **Tauri / web tabanlı arayüz:** WebKitGTK bağımlılığı getirir, bu da yukarıdaki
  "temiz makinede çalışsın" hedefiyle çelişir.
- **GTK4-rs:** Daha yerel bir görünüm verir, karşılığında derleme ve dağıtım
  karmaşıklığı artar. İleride ikinci bir arayüz olarak eklenebilir — çekirdek
  crate zaten arayüzden bağımsız.
- **`git` CLI çağırmak:** En hızlı yol, ama stderr ayrıştırmak kırılgan.

---

## 2. Mimari

```
confsync-core   (kütüphane, arayüzden bağımsız)
├── settings    Ayar modeli, varsayılan kaynak ve hariç tutma listeleri
├── paths       Mutlak yol <-> depo içi yol eşlemesi
├── scan        Dosya sistemi gezintisi + filtreleme
├── secrets     Sır sezgisi
├── manifest    Git'in tutamadığı üstveri (izinler, symlink hedefleri)
├── backup      Tara -> kopyala -> manifest -> commit
├── restore     Plan çıkar (kuru çalışma) -> uygula
├── gitrepo     libgit2 sarmalayıcısı
└── job         Arka plan işçisi (komut/olay kanalları)

confsync-gui    (ikili)
└── ui          eframe uygulaması, sekmeler
```

**Ayrım nedeni:** İş mantığının hiçbir parçası egui'ye bağlı değil. Bu sayede
aynı çekirdek üzerine ileride bir CLI ya da systemd timer ile çalışan bir
servis eklenebilir; ayrıca çekirdek GUI olmadan test edilebilir (mevcut testler
bunu yapıyor).

**Eşzamanlılık:** GUI iş parçacığı hiçbir zaman dosya sistemi veya ağ işlemi
yapmaz. `job::Worker` ayrı bir iş parçacığında çalışır; `Command` gönderilir,
`Event` alınır. Her olayda `egui::Context::request_repaint` tetiklenir. İptal
bayrağı `AtomicBool` ile paylaşılır.

---

## 3. Depo düzeni

```
<repo>/
├── README.md
├── .gitattributes            # * -text  (satır sonu dönüşümü yapılmasın)
└── profiles/
    └── <makine-adı>/
        ├── manifest.json
        └── files/
            ├── home/.bashrc              <- $HOME/.bashrc
            ├── home/.config/nvim/...     <- $HOME/.config/nvim/...
            └── root/etc/hosts            <- /etc/hosts
```

İki karar burada önemli:

1. **Profil dizinleri.** Aynı depoyu dizüstü ve masaüstü paylaşabilsin diye her
   makinenin kendi alanı var. Ortak dosyaları paylaşmak için ileride bir
   `profiles/_ortak/` katmanı eklenebilir (bkz. yol haritası).
2. **`home/` ve `root/` önekleri.** Mutlak yolu doğrudan saklasaydık depo
   yalnızca aynı kullanıcı adına geri yüklenebilirdi. Bu önek sayesinde
   `/home/ali/.bashrc` yedeği `/home/veli/.bashrc` olarak açılabilir; arayüzde
   hedef ev dizini değiştirilebilir.

---

## 4. Git'in kaybettiği bilgi: manifest

Git bir dosya hakkında yalnızca "çalıştırılabilir mi" bitini saklar. Yapılandırma
yedeklemesinde bu yetersiz: `~/.ssh/config` 0600 değilse SSH dosyayı reddeder,
`0755` olması gereken bir betik `0644` olarak geri gelirse çalışmaz.

Bu yüzden her yedekte `manifest.json` üretilir:

```json
{
  "version": 1,
  "profile": "dizustu",
  "source_home": "/home/ali",
  "tool_version": "0.1.0",
  "entries": [
    {
      "repo_path": "home/.ssh/config",
      "origin_path": "/home/ali/.ssh/config",
      "kind": "file",
      "mode": 384,
      "size": 210,
      "sha256": "…",
      "uid": 1000,
      "gid": 1000
    }
  ]
}
```

**Manifest belirlenimci olmalıdır.** İlk taslakta manifest'e `created_at` alanı
konmuştu; sonuç, hiçbir dosya değişmese bile her yedeğin yeni bir commit
üretmesiydi. Alan kaldırıldı, girdiler yola göre sıralanıyor ve dizin gezintisi
`sort_by_file_name()` ile sabitlendi. "Ne zaman yedeklendi" bilgisi zaten git
commit tarihinde var. (Bu hatayı entegrasyon testi yakaladı.)

**Sembolik bağlantılar** depoya kopyalanmaz; hedefleri manifest'e yazılır ve
geri yüklemede yeniden kurulur. Aksi halde `~/.config/nvim -> ~/dotfiles/nvim`
gibi bir bağlantı, hedefin tüm içeriğini ikinci kez yedeklerdi.

**Sahiplik (uid/gid)** kaydedilir ama geri yüklemede uygulanmaz — `chown` root
gerektirir. Farklı uid'li bir makineye açılırken sayısal uid'i körlemesine
uygulamak zarar verir.

---

## 5. Sır (secret) koruması

Yapılandırma dizinleri sık sık token ve özel anahtar barındırır; bunları uzak
bir depoya push etmek en gerçekçi hasar senaryosu. Üç katmanlı savunma:

1. **Varsayılan hariç tutma listesi** — `**/.ssh/id_*` (ama `!**/.ssh/id_*.pub`),
   `*.pem`, `*.key`, `.aws/credentials`, `.netrc`, `.npmrc` vb.
2. **İçerik sezgisi** — PEM özel anahtar başlıkları ve `api_key = "…"` biçimli
   gerçekçi değer atamaları. Yer tutucular (`<your-key>`, `changeme`, `$VAR`)
   ve yorum satırları elenir.
3. **Görünürlük** — atlanan her dosya Genel Bakış sekmesinde gerekçesiyle
   listelenir. Sessizce atlamak, kullanıcının yedeğinin eksik olduğunu
   fark etmemesine yol açar.

**Denenip vazgeçilen kural:** "izinleri 0600 olan dosya sırdır". Test sırasında
bunun `~/.config/uygulama/settings.toml` gibi tamamen meşru dosyaları elediği
görüldü. İzin biti tek başına kanıt değil; kural kaldırıldı.

Bu sezgiler **kesin değildir**. Uzak depo herkese açıksa, ilk push öncesi
atlananlar listesinin gözden geçirilmesi gerekir.

---

## 6. Geri yükleme: önce plan, sonra uygulama

Geri yükleme, kullanıcının makinesindeki dosyaların üzerine yazar; yani veri
kaybettirebilecek tek işlem. Bu yüzden iki aşamalı:

1. `restore::plan()` — diske hiçbir şey yazmaz. Her madde için karar üretir:
   `oluşturulacak` / `üzerine yazılacak` / `değişmedi` (SHA-256 karşılaştırması) /
   `çakışma` (ör. dosya beklenirken dizin var) / `depoda bulunamadı`.
2. Kullanıcı listeyi görür, tek tek işaret kaldırabilir, onaylar.
3. `restore::apply()` — üzerine yazılacak her dosyanın kopyasını
   `~/.local/share/confsync/rollback/<zaman-damgası>/` altına alır, sonra yazar.

Yazma işlemi geçici dosyaya kopyalayıp `rename` ile yerine taşıma biçiminde
yapılır; süreç ortada kesilirse yarım dosya kalmaz.

---

## 7. Bilinen sınırlar

| Konu | Durum |
|---|---|
| `/etc` altı | Okuma çoğu dosyada serbest, **yazma root gerektirir**. Şu an geri yükleme bu dosyalarda başarısız olur ve hata listelenir. Çözüm: `pkexec` ile ayrıcalıklı yardımcı süreç (yol haritasında). |
| Boş dizinler | Git boş dizin tutamaz. Manifest'te `kind: "dir"` desteği var ama tarayıcı henüz üretmiyor. |
| Şifreleme | Yok. Sırlar için çözüm "yedekleme" değil "hariç tutma". `age`/`git-crypt` entegrasyonu yol haritasında. |
| Ayrışmış dallar | `pull` yalnızca fast-forward yapar; ayrışma varsa hata verir ve birleştirmeyi kullanıcıya bırakır. Bir dotfile aracının otomatik merge denemesi tehlikeli. |
| Büyük dosyalar | Varsayılan 5 MiB üstü atlanır. Git deposunun şişmesini engeller. |
| Windows/macOS | Hedeflenmiyor. Kod Unix izinlerine ve `/` düzenine bağlı. |

---

## 8. Yol haritası

**v0.1 — bu iskelet**
- Ayar yönetimi, kaynak/hariç tutma düzenleme, yerel yedek + commit, plan
  tabanlı geri yükleme, geçmiş görünümü, push/pull.

**v0.2**
- İlk çalıştırmada kurulum sihirbazı (depo seç → uzak adres → kaynakları onayla).
- Yedek öncesi "ne değişecek" ön izlemesi (dosya bazlı diff).
- Geçmişteki bir commit'e dönerek geri yükleme.
- `pkexec` ile sistem dosyaları (`/etc`) desteği.

**v0.3**
- `profiles/_ortak/` katmanı ve profil devralma.
- `age` ile seçili dosyaların şifrelenerek saklanması.
- Zamanlanmış yedek (systemd user timer üretimi).
- Paketleme: AppImage ve Flatpak.

---

## 9. Doğrulama durumu

- `confsync-core`: **derleniyor, 10 test geçiyor** (birim testleri + uçtan uca
  yedekle/boz/geri-yükle turu). Test ortamında rustc 1.75 kullanıldı; bu yüzden
  bazı bağımlılıklar geçici olarak eski sürümlere sabitlendi. Depodaki
  `Cargo.toml` sabitleme içermez.
- `confsync-gui`: **derleme doğrulanmadı.** egui 0.31, rustc 1.81+ ister; test
  ortamında o sürüm yoktu. Kod gözden geçirildi (özellikle `restore` görünümündeki
  ödünç alma çakışması düzeltildi), ancak ilk `cargo build` sonrası küçük API
  düzeltmeleri beklenmelidir.
