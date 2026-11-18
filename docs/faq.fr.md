# RealmKeeper : FAQ et dépannage

[English version](faq.md)

Chaque message d'erreur du launcher a un lien **Aide** vers la section
correspondante ci-dessous. Si rien ici ne t'aide, utilise **Signaler un
problème** en bas du launcher. Clique d'abord sur **Copier le diagnostic**
et colle-le dans le signalement : tes dossiers personnels y sont masqués.

- [Avant de commencer](#start)
- [Le dossier du jeu](#client)
- [Emplacement d'installation et espace disque](#location)
- [Antivirus et fichiers serveur manquants](#antivirus)
- [Ports et autres serveurs](#ports)
- [Téléchargements et Internet](#network)
- [Un changement interrompu ou échoué](#operations)
- [Des serveurs qui ne démarrent pas](#servers)
- [Sauvegardes et restaurations](#backups)
- [Plusieurs royaumes, changer de PC](#realms)
- [Jouer en réseau local](#lan)
- [Panneau MJ et comptes](#gm)
- [Autre chose](#other)

<a id="start"></a>
## Avant de commencer

- **De quoi ai-je besoin ?** Windows 10 ou 11 (64 bits), environ 10 Go
  libres par royaume, et ton propre client World of Warcraft pour la
  version voulue (3.3.5a, build 12340, pour Wrath of the Lich King). Le
  launcher ne télécharge ni ne partage jamais le client du jeu.
- **Faut-il les droits administrateur ?** Non. Tout s'installe dans ton
  dossier utilisateur ou dans le dossier choisi. Seule la règle de
  pare-feu facultative pour le jeu en réseau local demande une élévation.
- **Le jeu est-il modifié ?** Le launcher écrit l'adresse du royaume et
  n'ajoute que les fichiers des options de module que tu actives (par
  exemple un patch client ou l'addon MultiBot). Ces fichiers sont retirés
  quand tu désactives l'option. Tes propres fichiers sont mis de côté, puis
  remis en place.

<a id="client"></a>
## Le dossier du jeu

- **« Aucun jeu trouvé dans ce dossier »** : choisis le dossier qui contient
  `Wow.exe`, pas son dossier `Data` ni un raccourci. Le launcher cherche
  aussi le jeu tout seul sur ce PC et propose ce qu'il trouve (**Utiliser
  celui-ci**).
- **« Ce jeu est en build … »** : chaque royaume demande une version exacte
  du client (la 3.3.5a est le build 12340). Les autres builds ne peuvent pas
  se connecter.
- **« Le launcher ne peut pas écrire dans ce dossier de jeu »** : les jeux
  installés sous `C:\Program Files` sont protégés par Windows. Déplace le
  dossier du jeu ailleurs (par exemple `C:\Jeux`), puis choisis-le de
  nouveau.
- **« Un fichier de ton dossier de jeu serait remplacé »** : les détails
  donnent son nom. Sors-le du dossier du jeu (garde-le s'il est à toi),
  puis réessaie.
- **« Un fichier du jeu est utilisé »** : ferme le jeu, puis réessaie.

<a id="location"></a>
## Emplacement d'installation et espace disque

- **Quel dossier choisir ?** Un dossier vide sur un disque interne au
  format NTFS (le format habituel de Windows), par exemple
  `D:\RealmKeeper`.
- **« … est synchronisé par OneDrive / Dropbox / Google Drive »** : une
  base de données dans un dossier synchronisé finit corrompue, car les
  fichiers sont envoyés pendant que le serveur les écrit. Windows met
  souvent « Documents » et « Bureau » dans OneDrive : choisis un dossier
  en dehors.
- **« … ne peut pas contenir les fichiers serveur »** : les clés USB en
  FAT32/exFAT et la plupart des lecteurs réseau ne gèrent pas les liens de
  dossiers dont les serveurs ont besoin. Choisis un disque interne NTFS.
- **« Espace disque insuffisant »** : le message donne l'espace
  nécessaire et l'espace libre. Libère de la place, ou choisis un autre
  emplacement avant la première installation. Le launcher vérifie avant
  de télécharger quoi que ce soit.
- **J'ai renommé ou déplacé le dossier.** Au démarrage suivant, le
  launcher te demande de le retrouver. Un royaume déplacé affiche
  **Mettre à jour les chemins** dans son onglet Accueil.

<a id="antivirus"></a>
## Antivirus et fichiers serveur manquants

Les programmes serveur ne sont pas des applications connues, donc un
antivirus les met parfois en quarantaine (`worldserver.exe`,
`authserver.exe`, `legacy.dll`, …).

1. Ouvre ton antivirus (Sécurité Windows › Protection contre les virus et
   menaces › Historique de protection) et restaure les fichiers retirés.
   Tu peux aussi sauter cette étape.
2. Ajoute une **exclusion** pour le dossier de RealmKeeper (Sécurité
   Windows › Protection contre les virus et menaces › Gérer les paramètres
   › Exclusions).
3. Dans le launcher, clique sur **Réparer** (Accueil › Entretien). Il
   télécharge et vérifie de nouveau les fichiers serveur. Tes personnages
   et comptes sont conservés.

Le launcher ne télécharge que des fichiers dont il a vérifié la signature
ou l'empreinte.

<a id="ports"></a>
## Ports et autres serveurs

Un royaume écoute sur les ports 3724 (connexion), 8085 (monde) et 7878
(SOAP), plus un port privé pour sa base de données, à partir de 3307.

- **« Le port … est utilisé par … »** : un autre programme tient le port.
  C'est souvent un autre serveur WoW, ou un second royaume de ce launcher
  encore allumé. Ferme-le, puis réessaie. Un seul royaume tourne à la fois.
- **« Aucun port réseau libre »** : ferme les autres serveurs de bases de
  données, puis réessaie.

<a id="network"></a>
## Téléchargements et Internet

- **« Un téléchargement a échoué »** : vérifie ta connexion, puis relance
  la même action. Les téléchargements reprennent là où ils s'étaient
  arrêtés et sont vérifiés à la fin.
- **« La liste des versions du serveur n'a pas pu être vérifiée »** : la
  liste signée des versions du serveur n'a pas pu être récupérée, ou sa
  signature est fausse. Réessaie plus tard. Si un proxy ou un pare-feu
  filtre GitHub, autorise `github.com` et `objects.githubusercontent.com`.
- **Le jeu a-t-il besoin d'Internet ?** Non. Une fois installé, un royaume
  tourne hors ligne. L'installation, les mises à jour et l'import ont
  besoin d'Internet.

<a id="operations"></a>
## Un changement interrompu ou échoué

Les installations, mises à jour, changements de modules, restaurations et
imports se font étape par étape. Chaque étape est enregistrée, donc rien
n'est perdu quand l'une s'arrête.

- **« Annulé » / « Une étape a échoué » / launcher fermé / coupure de
  courant** : relance la même action, ou clique sur **Reprendre** dans
  l'onglet Accueil. Elle continue avec le même plan.
- **« Le changement a échoué et a été annulé »** : tout est comme avant.
  Réessaie. Si ça échoue encore, signale le problème avec les détails
  techniques.
- **« … n'a pas pu être entièrement annulé »** : ne réessaie pas. Signale
  le problème avec les détails techniques et le diagnostic. Les sauvegardes
  faites avant le changement sont dans l'onglet Sauvegardes.
- **Mon PC peut-il se mettre en veille pendant une installation ?** Le
  launcher le garde éveillé tant qu'il travaille.

<a id="servers"></a>
## Des serveurs qui ne démarrent pas

- **« … le démarrage a échoué. Sa console indique pourquoi. »** : ouvre
  **Terminaux** en bas du launcher et lis les dernières lignes de ce
  serveur.
  - Une erreur de base de données : un autre programme utilise son port,
    ou le disque est plein.
  - Des fichiers manquants : voir [antivirus](#antivirus).
- **Le serveur de jeu est long à démarrer.** Avec 500 bots, il faut de 1 à
  4 minutes. La barre sous « Serveurs » montre où il en est.
- **« Le dossier de ce royaume a été renommé ou déplacé »** : clique sur
  **Mettre à jour les chemins** dans l'onglet Accueil.
- **Éteindre Windows avec les serveurs allumés** : le launcher les arrête
  d'abord proprement, donc le monde est sauvegardé. Windows affiche
  pourquoi il attend, jusqu'à une minute.

<a id="backups"></a>
## Sauvegardes et restaurations

- **Qu'est-ce qui est sauvegardé ?** Les comptes, les personnages et les
  bots : les données des joueurs. Le monde du jeu lui-même est reconstruit
  à partir de la version du serveur.
- **Quand ?** Une sauvegarde quotidienne est faite à l'arrêt des serveurs
  (si elle est activée), et une sauvegarde complète avant chaque mise à
  jour, réparation et restauration. Tu peux aussi cliquer sur
  **Sauvegarder maintenant**, même pendant que les serveurs tournent : les
  joueurs sont d'abord enregistrés, puis leurs données copiées sans arrêter
  le royaume.
- **Restaurer** remet les données des joueurs telles que la sauvegarde
  les contient (les serveurs doivent être arrêtés). L'état actuel est
  sauvegardé d'abord, donc une restauration peut être annulée.
- **« Ce fichier ne contient aucune donnée de joueur de ce royaume »** :
  le fichier vient d'un autre émulateur, ou ce n'est pas une sauvegarde.

<a id="realms"></a>
## Plusieurs royaumes, changer de PC

- **Dupliquer** (Accueil, au-dessus du royaume) copie un royaume arrêté
  avec ses modules, réglages, comptes et personnages. Utilise-le pour
  essayer d'autres modules sans risque. La copie prend sur le disque la
  taille de sa base de données (1 à 2 Go). Un seul royaume tourne à la
  fois.
- **Exporter** enregistre un royaume arrêté dans un fichier `.rkrealm` :
  modules, réglages, comptes et personnages, sans le monde du jeu, qui est
  reconstruit.
- **Importer un royaume**, sur l'autre PC, installe un nouveau royaume
  depuis ce fichier, puis remet ses comptes et personnages. Il faut
  Internet, comme pour une première installation.
- **« Ce royaume utilise …, que ce launcher ne propose pas »** : mets
  d'abord le launcher à jour sur ce PC.
- **« Ce fichier n'est pas un royaume exporté par RealmKeeper »** :
  le fichier est abîmé ou d'un autre type. Exporte-le de nouveau.
- Un export contient les noms des comptes et la vérification de leurs mots
  de passe (pas les mots de passe eux-mêmes). Partage-le seulement comme
  une sauvegarde.

<a id="lan"></a>
## Jouer en réseau local

- L'hôte installe le royaume avec **Réseau local**. Les amis utilisent
  **Rejoindre une partie sur le réseau local** et saisissent l'adresse de
  l'hôte (affichée dans son onglet Accueil, par exemple `192.168.1.20`).
- Si les amis ne peuvent pas se connecter, le pare-feu Windows de l'hôte
  bloque sans doute les ports 3724 et 8085. Dans l'onglet Accueil de
  l'hôte, **Autoriser dans le pare-feu** ajoute une règle pour les serveurs
  de ce royaume seulement (Windows demande une fois les droits
  d'administrateur) ; **Retirer la règle** l'enlève.
- La règle s'applique aux réseaux **privés**. Si le launcher indique que
  Windows considère ton réseau comme public, ouvre Paramètres Windows >
  Réseau et Internet > ta connexion, et choisis **Réseau privé**.
- Les options de module qui ajoutent des fichiers au client (patchs,
  addons) doivent aussi être installées dans le dossier de jeu des amis.

<a id="gm"></a>
## Panneau MJ et comptes

- Crée ton compte dans l'onglet **Compte**. Un niveau MJ au-dessus de 0
  donne accès aux commandes MJ en jeu. Le nouveau niveau s'applique à ta
  prochaine connexion.
- L'onglet **Compte**, partie **Maître du jeu**, envoie des commandes toutes prêtes
  (téléporter, niveau, ressusciter, envoyer un objet ou de l'or, annonce)
  pendant que les serveurs tournent. Les valeurs sont vérifiées d'abord.

<a id="other"></a>
## Autre chose

Utilise **Signaler un problème** en bas du launcher. Clique d'abord sur
**Copier le diagnostic** et colle-le dans le signalement : il donne les
versions, les royaumes et les dernières lignes des journaux, avec tes
dossiers personnels masqués.
