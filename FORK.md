# HatchZip Fork : HatchZip avec une interface façon WinRAR

Ce fork ajoute à [HatchZip](https://github.com/xevrion/hatchzip) ce qui lui manquait pour remplacer WinRAR au quotidien. Le moteur d'extraction sur place d'origine reste intact.

## Ce que le fork ajoute

| | |
|---|---|
| Création d'archives | ZIP (Deflate ou stockage) et 7z (LZMA2 ou stockage), 6 niveaux de compression, mot de passe AES-256, noms de fichiers chiffrés (7z), découpage en volumes (`.zip.001`, `.7z.001`…), commentaire ZIP |
| Archives « extractibles sur place » | Les 7z sont écrits en blocs solides de 256 Mo par défaut (réglable), pour que HatchZip puisse les ré-extraire plus tard avec très peu d'espace libre |
| Options d'archivage | Supprimer les fichiers après l'archivage, tester l'archive après création, une archive par élément, enregistrer les réglages par défaut, historique des noms d'archive, pause et annulation (l'archive incomplète est supprimée) |
| Menu clic droit Windows 11 | Entrée « HatchZip » au premier niveau du menu, sans passer par « Afficher d'autres options », avec des libellés comme WinRAR : *Ajouter à « Jeu.zip »*, *Extraire vers « Jeu\ »*… |
| Menu clic droit classique | Aussi sur les dossiers et tous les fichiers, en français si Windows est en français |
| Fenêtre principale façon WinRAR | Barre d'outils Ajouter, Extraire vers, Tester, Afficher, Supprimer, Infos ; barre d'adresse ; liste triable par colonne ; sélection multiple (Ctrl, Maj, Ctrl+A) ; menu clic droit dans la liste ; barre d'état avec totaux |
| Navigation dans les archives | Double-clic sur une archive pour voir son contenu, dossiers compris ; « Afficher » ouvre un fichier d'un ZIP ou d'un 7z sans tout extraire |
| Petits plus | Favoris, recherche dans la liste, suppression vers la Corbeille, informations sur l'archive ou la sélection, historique des dossiers d'extraction, extraction de plusieurs archives chacune dans son dossier |
| Français | Toute l'interface est traduite en français |

Limites : on ne peut pas créer de `.rar` (format propriétaire), ni ajouter ou supprimer des fichiers dans une archive existante. Les messages d'erreur venant du moteur restent en anglais.

## Compiler

Rust est installé dans `A:\Dev\rust`. Depuis ce dossier :

```powershell
cargo build --release --features rar
```

## Installer

```powershell
powershell -ExecutionPolicy Bypass -File packaging\windows\install.ps1
```

Le script copie le programme dans `%LOCALAPPDATA%\Programs\HatchZip Fork`, ajoute le menu clic droit classique et un raccourci dans le menu Démarrer.

Pour l'entrée au premier niveau du menu Windows 11, il faut une seule fois faire confiance au certificat créé sur ce PC. Lancez **en administrateur** :

```powershell
powershell -ExecutionPolicy Bypass -File packaging\windows\trust-certificate.ps1
```

puis relancez `install.ps1`.

Pour tout retirer : `packaging\windows\uninstall.ps1`.

Si le HatchZip d'origine est aussi installé, désinstallez-le : les deux utilisent les mêmes entrées du menu clic droit.
