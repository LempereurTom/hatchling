<h1 align="center">HatchZip Fork</h1>

<p align="center">
  <strong>HatchZip, avec une interface et un menu clic droit façon WinRAR.</strong><br>
  Extraction sur place des grosses archives + création d'archives ZIP et 7z, gratuit et open source.
</p>

<p align="center">
  <a href="#licence"><img src="https://img.shields.io/badge/licence-MIT%20OU%20Apache--2.0-blue.svg" alt="Licence MIT ou Apache-2.0" /></a>
  <img src="https://img.shields.io/badge/plateforme-Windows%2010%20%7C%2011-blue.svg" alt="Windows 10 et 11" />
  <img src="https://img.shields.io/badge/langue-fran%C3%A7ais-blue.svg" alt="Interface en français" />
</p>

> [!IMPORTANT]
> Ceci est un **fork non officiel** de [HatchZip](https://github.com/xevrion/hatchzip), créé par [xevrion](https://github.com/xevrion) et ses contributeurs. Il n'est ni affilié au projet d'origine, ni soutenu par lui. Les bugs de ce fork se signalent ici, pas sur le dépôt d'origine.

*English summary below.*

## À quoi ça sert

[HatchZip](https://github.com/xevrion/hatchzip) a une idée géniale : il **extrait une archive sur place**. Il la lit depuis la fin et la raccourcit au fur et à mesure que les fichiers sortent. Résultat : pour un jeu de 180 Go qui fait 200 Go une fois extrait, il faut un peu plus de 200 Go libres, contre 380 Go avec WinRAR ou 7-Zip.

Mais HatchZip ne fait que de l'extraction, et son interface ne ressemble à rien de connu. Ce fork lui ajoute ce qu'il faut pour **remplacer WinRAR au quotidien**, sans toucher au moteur d'extraction :

- **créer des archives** ZIP et 7z ;
- un **menu clic droit** au premier niveau de Windows 11, sur les fichiers, les dossiers et les archives ;
- une **fenêtre principale organisée comme celle de WinRAR**, pour s'y retrouver tout de suite ;
- toute l'interface **en français**.

## Ce que le fork ajoute

### Le menu clic droit

L'entrée « HatchZip » apparaît directement dans le menu de Windows 11, sans passer par « Afficher d'autres options ». Les libellés reprennent le nom de l'élément, comme WinRAR :

| Entrée | Ce qu'elle fait |
|---|---|
| Extraire les fichiers… | Demande un dossier de destination, puis extrait |
| Extraire ici | Extrait à côté de l'archive. Si l'archive contient plusieurs éléments à sa racine, ils vont dans un dossier à son nom, pour ne pas se mélanger avec le reste |
| Extraire ici (garder l'archive) | Pareil, mais l'archive est conservée intacte |
| Extraire vers « Nom\ » | Extrait dans un nouveau dossier au nom de l'archive |
| Tester l'archive | Vérifie chaque fichier sans rien écrire |
| Ouvrir avec HatchZip | Ouvre l'archive dans la fenêtre principale pour voir son contenu |
| Ajouter à l'archive… | Ouvre la fenêtre « Nom et paramètres de l'archive » |
| Ajouter à « Nom.zip » / « Nom.7z » | Crée l'archive tout de suite, avec les réglages par défaut |
| Ajouter chaque élément à son propre .zip | Avec plusieurs éléments sélectionnés : un .zip par élément |

Les entrées d'extraction n'apparaissent que sur les archives. Le menu classique (« Afficher d'autres options ») est aussi rempli, en français si Windows est en français.

### La création d'archives

- **ZIP** (Deflate ou stockage) et **7z** (LZMA2 ou stockage), avec 6 niveaux de compression, de *Stockage* à *Meilleure*.
- **Mot de passe AES-256**, et chiffrement des noms de fichiers en 7z.
- **Découpage en volumes** : 100 Mo, 700 Mo (CD), 4 Go (FAT32), 4,7 Go (DVD), 25 Go (Blu-ray) ou une taille personnalisée.
- **Options d'archivage** : supprimer les fichiers après l'archivage, tester l'archive après sa création, une archive par fichier, commentaire ZIP.
- **Archives prêtes pour l'extraction sur place** : les 7z sont écrits en blocs solides de 256 Mo par défaut. HatchZip pourra donc les ré-extraire plus tard avec seulement environ un bloc d'espace libre en plus. Avec un seul gros bloc, ce serait impossible.
- **Pause et annulation** : en cas d'annulation, l'archive incomplète est supprimée et les fichiers d'origine ne sont pas touchés.
- **Réglages par défaut** enregistrables, et historique des noms d'archive.

### La fenêtre principale

- **Barre d'outils** : Ajouter, Extraire vers, Tester, Afficher, Supprimer, Infos.
- **Barre d'adresse**, liste des lecteurs, bouton dossier parent.
- **Liste triable** par nom, taille, type ou date ; sélection multiple (Ctrl, Maj, Ctrl+A) ; clic droit dans la liste ; navigation au clavier.
- **Navigation dans les archives** : double-clic sur une archive pour voir son contenu, dossiers compris. « Afficher » ouvre un fichier d'un ZIP ou d'un 7z sans extraire tout le reste.
- **Petits plus** : favoris, recherche dans la liste, suppression vers la Corbeille, informations sur l'archive (taux de compression, chiffrement…), historique des dossiers d'extraction, extraction de plusieurs archives chacune dans son dossier.

### Ce qui vient de HatchZip et ne change pas

- Extraction sur place de **ZIP, 7z, RAR, tar, tar.gz, tar.xz** et des archives fractionnées.
- Vérification de l'espace disque **avant** de toucher à quoi que ce soit, avec une marge de sécurité réglable.
- Reprise après un plantage ou un arrêt, et « Restaurer l'archive » pour revenir à une archive normale.
- Vérification par fichier `.sfv`, gestion des fichiers déjà présents (Remplacer / Ignorer / Conserver les deux).

Le fonctionnement du moteur est expliqué dans [docs/how-it-works.md](docs/how-it-works.md), et la documentation d'origine dans [docs/UPSTREAM-README.md](docs/UPSTREAM-README.md).

### Limites

- On ne peut pas **créer** de `.rar` : le format est propriétaire. Les RAR s'ouvrent et s'extraient normalement.
- On ne peut pas encore ajouter ou retirer des fichiers dans une archive existante.
- Les messages d'erreur du moteur restent en anglais.
- Pas de version compilée publiée pour l'instant : il faut compiler soi-même (voir ci-dessous).

## Installation (Windows 10 et 11)

### 1. Compiler

Il faut [Rust](https://rustup.rs) et les outils C++ de Visual Studio (Build Tools), ainsi que le SDK Windows pour le menu Windows 11. La version de Rust est fixée par `rust-toolchain.toml` et s'installe toute seule.

```powershell
git clone https://github.com/LempereurTom/hatchzip-fork.git
cd hatchzip-fork
cargo build --release --features rar
```

### 2. Installer

```powershell
powershell -ExecutionPolicy Bypass -File packaging\windows\install.ps1
```

Le script installe tout pour l'utilisateur courant, sans droits administrateur :
- le programme dans `%LOCALAPPDATA%\Programs\HatchZip Fork` ;
- un raccourci « HatchZip » dans le menu Démarrer ;
- le menu clic droit classique.

### 3. Menu Windows 11 au premier niveau (une seule fois)

Windows 11 n'affiche une entrée au premier niveau du menu que si elle vient d'un paquet signé. Le script d'installation crée un certificat sur votre PC pour le signer. Pour que Windows l'accepte, lancez une fois **en administrateur** :

```powershell
powershell -ExecutionPolicy Bypass -File packaging\windows\trust-certificate.ps1
```

puis relancez `install.ps1`. Le certificat est créé localement et sa clé privée ne quitte pas votre PC.

### Désinstaller

```powershell
powershell -ExecutionPolicy Bypass -File packaging\windows\uninstall.ps1
```

Si le HatchZip d'origine est aussi installé, mieux vaut le désinstaller : les deux utilisent les mêmes entrées du menu clic droit.

## Pour les développeurs

| Dossier | Contenu |
|---|---|
| `crates/core` | Le moteur. `create.rs` (nouveau) écrit les archives ZIP et 7z ; `peek.rs` (nouveau) lit un seul fichier d'une archive |
| `crates/gui` | L'application. `explorer.rs` et `ui/explorer.slint` : la fenêtre principale façon WinRAR ; `add_window.rs` et `ui/add-window.slint` : « Nom et paramètres de l'archive » |
| `crates/shellext` | Nouveau : le menu clic droit de Windows 11 (serveur COM `IExplorerCommand`) |
| `crates/shell` | Le menu clic droit classique, étendu aux dossiers et à tous les fichiers |
| `crates/cli` | La commande `hatchzip`, avec une nouvelle sous-commande `create` |
| `packaging/windows` | Les scripts d'installation et le paquet du menu Windows 11 |

```powershell
cargo test --workspace --features rar
cargo clippy --workspace --all-targets --features rar -- -D warnings
```

## English summary

**HatchZip Fork** is an unofficial fork of [HatchZip](https://github.com/xevrion/hatchzip), the in-place archive extractor, not affiliated with the original project. It keeps HatchZip's engine and adds what was missing to use it instead of WinRAR on Windows:

- creating **ZIP and 7z** archives (AES-256, split volumes, delete files after archiving, test after archiving, bounded solid blocks so the result can be extracted in place later);
- a **Windows 11 top-level context menu** entry, through an `IExplorerCommand` shell extension and a sparse package, plus the classic menu on folders and all files;
- a **WinRAR-style main window**: toolbar, address bar, sortable multi-select list, browsing inside archives, viewing a single file from a ZIP or 7z, favorites, Recycle Bin delete, archive info;
- a complete **French translation**.

Build with `cargo build --release --features rar`, then run `packaging\windows\install.ps1`.

## Licence

Comme le projet d'origine, au choix [MIT](LICENSE-MIT) ou [Apache 2.0](LICENSE-APACHE). La prise en charge des RAR compile le code source C++ d'UnRAR, sous sa propre [licence](LICENSE-UNRAR), qui autorise l'extraction mais pas la création de RAR.

Tout le mérite du moteur d'extraction sur place revient à [xevrion](https://github.com/xevrion) et aux contributeurs de [HatchZip](https://github.com/xevrion/hatchzip).
