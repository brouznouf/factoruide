// Description of every planner setting, used to generate the settings form.
import type { Params } from "./api";

export type Field =
  | { key: keyof Params; kind: "number"; label: string; help?: string; unit?: string; min?: number; max?: number; step?: number; scale?: number }
  | { key: keyof Params; kind: "bool"; label: string; help?: string }
  | { key: keyof Params; kind: "select"; label: string; help?: string; options: { value: string; label: string }[] };

export interface Section {
  title: string;
  help?: string;
  /** For testing the planner: folded until opened. */
  expert?: boolean;
  fields: Field[];
}

export const sections: Section[] = [
  {
    title: "Contenu",
    fields: [
      { key: "dungeons", kind: "bool", label: "Faire les donjons rentables en groupe", help: "Un donjon est fait, avec ses quêtes, quand il fait gagner du temps." },
      { key: "dungeon_pickup_ahead", kind: "number", label: "Prendre les quêtes de donjon jusqu'à N niveaux avant", help: "Prises en passant à côté du donneur, gardées jusqu'au donjon.", unit: "niveaux", min: 0, max: 20, step: 1 },
      { key: "allow_elite", kind: "bool", label: "Quêtes élites (groupe)", help: "Quêtes dont les objectifs sont des mobs élites hors donjon." },
      { key: "all_class_quests", kind: "bool", label: "Toutes les quêtes de classe", help: "Impose toutes les quêtes de ta classe, pas seulement celles qui donnent un sort : plus long, mais rien n'est laissé de côté." },
      { key: "solo_elite_kills", kind: "number", label: "Élites seules (en solo)", help: "Sans les quêtes élites, garde celles qui demandent au plus ce nombre de kills d'élite (Hogger, Tharil'zun, Mor'Ladim...). 0 = aucune.", min: 0, max: 5, step: 1 },
      { key: "elite_kill_factor", kind: "number", label: "Durée d'un kill d'élite", help: "Par rapport à un mob normal du même niveau (les élites donnent 2× l'XP).", unit: "×", min: 1, max: 6, step: 0.5 },
      {
        key: "progression",
        kind: "select",
        label: "Profil de progression",
        help: "Jusqu'où le personnage va au-dessus de sa puissance (niveau, équipement, sorts).",
        options: [
          { value: "cautious", label: "Prudent (survie) : groupes de mobs jamais à plus d'un niveau au-dessus de vous" },
          { value: "normal", label: "Normal : quêtes jaunes, jamais d'orange (recommandé)" },
          { value: "risky", label: "Risqué : quêtes orange, élites limites" },
        ],
      },
      { key: "pull_levels", kind: "number", label: "Niveaux demandés par mob en plus dans un pull", help: "1 : des mobs qui vont par deux demandent un niveau de plus que des mobs isolés.", min: 0, max: 3, step: 0.5 },
      { key: "elite_pull_levels", kind: "number", label: "Niveaux demandés par élite en plus dans un camp", help: "1 : un camp où l'on tire trois élites à la fois demande deux niveaux de plus qu'un élite isolé.", min: 0, max: 4, step: 0.5 },
      { key: "max_quest_above", kind: "number", label: "Quêtes au-dessus du niveau visé prises en compte", help: "Jusqu'à N niveaux au-dessus du niveau visé : prises en route, rendues avant si c'est rentable.", min: 0, max: 60, step: 1 },
      { key: "pvp_quests", kind: "bool", label: "Quêtes de champ de bataille (PvP)", help: "Marques d'honneur et objectifs de champ de bataille, comptés à la durée des parties." },
      { key: "pvp_mark_time", kind: "number", label: "Temps de champ de bataille par marque ou objectif", help: "Durée de champ de bataille comptée pour chaque marque d'honneur ou objectif de quête PvP.", unit: "min", scale: 60, min: 5, max: 60, step: 5 },
      { key: "crowd_event_time", kind: "number", label: "Attente d'une escorte ou d'un événement", help: "Attente la plus longue pour une escorte ou un événement de quête, sur un serveur plein (moins selon le monde sur le serveur).", unit: "min", scale: 60, min: 0, max: 60, step: 5 },
      { key: "crowd_players", kind: "number", label: "Joueurs sur un même spot quand le serveur est plein", help: "Ils se partagent les mobs et les objets : un petit spot en laisse moins à chacun.", min: 0, max: 10, step: 0.5 },
      { key: "respawn_time", kind: "number", label: "Réapparition d'un mob ou d'un objet", help: "Pour les mobs et objets dont le temps de réapparition est inconnu : quand un objectif demande plus de mobs que son spot n'en a, le guide compte l'attente de leur retour.", unit: "min", scale: 60, min: 0, max: 15, step: 0.5 },
      { key: "dynamic_respawn", kind: "number", label: "Réapparitions plus rapides avec du monde", help: "Le jeu fait revenir mobs et objets plus vite quand il y a du monde : 50 % = avec 3 autres joueurs sur le spot, ils reviennent 2,5 fois plus vite. 0 % : réapparitions fixes, chacun attend son tour ; 100 % : chacun attend comme s'il était seul.", unit: "%", scale: 0.01, min: 0, max: 100, step: 10 },
      { key: "min_efficiency", kind: "number", label: "Rentabilité minimum d'une quête", help: "Par rapport au farm : 50 % = une quête doit rapporter au moins la moitié de l'XP/heure du farm.", unit: "%", scale: 0.01, min: 0, max: 300, step: 10 },
      { key: "quest_log_size", kind: "number", label: "Taille du journal de quêtes", help: "Quêtes que le journal contient à la fois (40 sur WoW Forever, 20 sur Classic) : le guide n'en porte jamais plus.", min: 5, max: 40, step: 1 },
    ],
  },
  {
    title: "Rythme de jeu",
    help: "Ajustez à votre façon de jouer : un joueur rapide tue plus vite et se repose moins.",
    fields: [
      { key: "kill_time", kind: "number", label: "Temps par mob de quête", unit: "s", min: 5, max: 60, step: 1, help: "Recherche et repos compris, mob de votre niveau, avec un équipement de base et tous les sorts." },
      { key: "grind_kill_time", kind: "number", label: "Temps par mob en farm", help: "Temps par mob quand le guide farme sans quête : plus de recherche et de repos qu'un mob de quête.", unit: "s", min: 10, max: 90, step: 1 },
      { key: "object_time", kind: "number", label: "Temps par objet à ramasser", help: "Temps pour utiliser ou ramasser un objet de quête (coffre, plante, caisse…).", unit: "s", min: 2, max: 60, step: 1 },
      { key: "gather_overlap", kind: "number", label: "Récolte entre deux kills", help: "Part du temps d'un kill (repos, recherche du mob suivant) pendant laquelle on ramasse les objets de quête autour : une étape de récolte juste avant ou après une étape de kills au même endroit se fait en partie pendant les kills. 0 = l'une après l'autre.", unit: "%", scale: 0.01, min: 0, max: 80, step: 5 },
      { key: "objective_alternatives", kind: "number", label: "Autres endroits par objectif", help: "Quand les mobs ou objets d'un objectif sont à plusieurs endroits, jusqu'à N autres groupes sont envisagés : le guide va au moins coûteux selon d'où il vient, où il va ensuite et la difficulté des mobs sur place. 0 = seulement l'endroit principal.", min: 0, max: 4, step: 1 },
      { key: "gather_radius", kind: "number", label: "Récolte et kills regroupés jusqu'à", help: "Distance entre une zone de récolte et une zone de mobs de quêtes en cours pour les faire ensemble.", unit: "m", min: 0, max: 400, step: 25 },
      { key: "stop_overhead", kind: "number", label: "Temps par étape (dialogue, loot)", help: "Temps fixe de chaque étape : parler au PNJ, lire la quête, ramasser le butin.", unit: "s", min: 0, max: 60, step: 1 },
      { key: "default_drop_chance", kind: "number", label: "Taux de drop par défaut", help: "Quand le vrai taux n'est pas connu.", unit: "%", scale: 0.01, min: 5, max: 100, step: 5 },
      { key: "min_start_chance", kind: "number", label: "Drop minimum pour démarrer une quête", help: "Les quêtes démarrées par un objet plus rare ne sont pas planifiées : on les prend s'il tombe.", unit: "%", scale: 0.01, min: 0, max: 50, step: 1 },
      { key: "default_kill_count", kind: "number", label: "Mobs à tuer par défaut", help: "Nombre de mobs compté quand les données du jeu ne disent pas combien en tuer.", min: 1, max: 30, step: 1 },
      { key: "default_item_count", kind: "number", label: "Objets à ramasser par défaut", help: "Nombre d'objets compté quand les données du jeu ne disent pas combien en ramasser.", min: 1, max: 30, step: 1 },
    ],
  },
  {
    title: "Déplacements",
    fields: [
      { key: "run_speed", kind: "number", label: "Vitesse de course", help: "Vitesse à pied, sans monture (7 dans le jeu).", unit: "m/s", min: 5, max: 10, step: 0.1 },
      { key: "mount_level", kind: "number", label: "Niveau de la monture", help: "Niveau où le personnage a sa monture : les trajets au sol vont plus vite ensuite.", min: 1, max: 60, step: 1 },
      { key: "mount_bonus", kind: "number", label: "Bonus de vitesse de la monture", help: "Vitesse en plus avec la monture (60 % pour la première).", unit: "%", scale: 0.01, min: 0, max: 100, step: 5 },
      { key: "detour", kind: "number", label: "Facteur de détour", help: "Les trajets sont plus longs que le chemin calculé sur le terrain (arbres, bâtiments, monstres à contourner). 1 : le chemin du terrain.", unit: "×", min: 1, max: 2, step: 0.05 },
      { key: "shortcuts", kind: "bool", label: "Raccourcis hors route", help: "Escalades et sauts connus (ex. Dun Morogh vers les Paluns par la montagne), déclarés dans overrides/passes.toml." },
      { key: "flight_speed", kind: "number", label: "Vitesse des vols", help: "Vitesse des vols (griffon, wyverne, hippogriffe).", unit: "m/s", min: 10, max: 60, step: 1 },
      { key: "flight_overhead", kind: "number", label: "Temps fixe par vol", help: "Temps ajouté à chaque vol : parler au maître de vol, décollage, atterrissage.", unit: "s", min: 0, max: 120, step: 5 },
      { key: "camp_radius", kind: "number", label: "Quêtes prises et rendues au passage jusqu'à", help: "À chaque étape, les quêtes du guide dont le PNJ est à cette distance sont rendues et prises tout de suite, comme dans un camp (0 = seulement à leur propre étape).", unit: "m", min: 0, max: 300, step: 10 },
      { key: "farm_on_way", kind: "bool", label: "Farmer en chemin", help: "À pied, les mobs normaux croisés sont tués au passage, comme le demandent la plupart des guides. Ce farm remplace le farm pur : il compte dans la part maximale de farm par niveau et s'arrête une fois celle-ci atteinte." },
      { key: "farm_corridor", kind: "number", label: "Mobs farmés en chemin jusqu'à", help: "Distance au trajet des mobs tués en chemin.", unit: "m", min: 5, max: 80, step: 5 },
      { key: "farm_share", kind: "number", label: "Part des mobs tués en chemin", help: "Part des mobs croisés à pied que le guide compte tuer : certains sont déjà morts ou hors de portée. Moins quand il y a du monde sur le serveur : les autres joueurs en tuent aussi. Les mobs des quêtes du journal n'en font pas partie, ils comptent pour leur objectif.", unit: "%", scale: 0.01, min: 0, max: 100, step: 5 },
      { key: "farm_below", kind: "number", label: "Farm en chemin : niveaux en dessous", help: "Les mobs plus bas que votre niveau moins N rapportent trop peu : ignorés.", unit: "niveaux", min: 0, max: 10, step: 1 },
      { key: "farm_above", kind: "number", label: "Farm en chemin : niveaux au-dessus", help: "Les mobs plus hauts que votre niveau plus N sont trop longs ou dangereux : contournés.", unit: "niveaux", min: 0, max: 5, step: 1 },
      { key: "flight_learn_radius", kind: "number", label: "Détour max pour apprendre un point de vol", help: "Un maître de vol plus proche que cette distance de la route est visité pour apprendre son point de vol.", unit: "m", min: 0, max: 1500, step: 50 },
      { key: "hearth_cooldown", kind: "number", label: "Recharge de la pierre de foyer", help: "Délai entre deux utilisations de la pierre de foyer (60 min dans le jeu).", unit: "min", scale: 60, min: 0, max: 60, step: 5 },
      { key: "hearth_cast", kind: "number", label: "Incantation de la pierre", help: "Durée de l'incantation de la pierre de foyer.", unit: "s", min: 0, max: 20, step: 1 },
      { key: "recall_cooldown", kind: "number", label: "Recharge du Rappel astral (chaman)", help: "Deuxième pierre de foyer du chaman, apprise au niveau 30.", unit: "min", scale: 60, min: 0, max: 60, step: 5 },
      { key: "teleport_cast", kind: "number", label: "Incantation des téléportations", help: "Téléportations du mage vers les capitales, et Rappel astral du chaman.", unit: "s", min: 0, max: 20, step: 1 },
      { key: "death_skip_time", kind: "number", label: "Durée d'une mort volontaire", help: "Mourir, libérer l'esprit et se faire ressusciter par l'ange de la mort (morts volontaires, jusqu'au niveau 10 et demi).", unit: "s", min: 10, max: 300, step: 5 },
      { key: "gold_weight", kind: "number", label: "Économiser l'or des vols", help: "Temps que vaut une pièce d'or dépensée en vols jusqu'au niveau 30, quand l'argent manque (0 = vols gratuits), moins ensuite car les quêtes rapportent plus : 60 min (défaut) jusqu'au niveau 30, 25 min au 40, 8 min au 60.", unit: "min", scale: 60, min: 0, max: 240, step: 10 },
      { key: "zone_change_weight", kind: "number", label: "Coût d'un changement de zone", help: "Temps compté en plus du trajet à chaque changement de zone : regroupe les quêtes par zone au lieu d'aller chercher quelques quêtes loin. 4 min par défaut.", unit: "min", scale: 60, min: 0, max: 15, step: 1 },
      { key: "abandon_weight", kind: "number", label: "Coût d'une quête abandonnée", help: "Temps compté pour chaque quête prise puis abandonnée (devenue grise, donjon fait sans elle) : la route la finit ou ne la prend pas, sauf si elle fait gagner plus. 10 min par défaut.", unit: "min", scale: 60, min: 0, max: 60, step: 1 },
      { key: "profession_quest_weight", kind: "number", label: "Coût d'une quête de métier laissée", help: "Avec des métiers : temps compté pour chaque quête de métier (Camping 101) que le guide ne fait pas : elle est prévue quand la route passe près de son maître, même grise, sans long détour. 5 min par défaut.", unit: "min", scale: 60, min: 0, max: 60, step: 1 },
      { key: "preferred_zone_weight", kind: "number", label: "Poids des zones privilégiées", help: "Part du temps passé hors des zones privilégiées comptée en plus : 0,5 (défaut) = une heure ailleurs compte comme une heure et demie. 0 = les zones privilégiées guident seulement le début de la recherche.", min: 0, max: 2, step: 0.1 },
      { key: "reputation_weight", kind: "number", label: "Valeur d'un point de réputation manquant", help: "Avec des objectifs de réputation (onglet principal) : temps de jeu que vaut chaque point qui manque à la fin du guide. 10 s (défaut) = une quête qui rapproche de 100 points de l'objectif vaut 17 minutes de plus.", unit: "s", min: 0, max: 60, step: 1 },
      { key: "flight_weight", kind: "number", label: "Poids des vols", help: "1 (défaut) = seul leur temps compte ; plus haut = marche ou réordonne la route plutôt que prendre un vol qui fait gagner peu de temps.", min: 1, max: 5, step: 0.25 },
    ],
  },
  {
    title: "Puissance du personnage",
    help: "La difficulté des combats suit la puissance (niveau + équipement + sorts appris) et l'XP suit le niveau : les quêtes qui donnent du bon équipement et les donjons rendent la suite plus rapide.",
    fields: [
      { key: "power_scale", kind: "number", label: "Poids de l'équipement et des sorts", help: "1 = le modèle tel quel ; 0 = la puissance est le niveau.", min: 0, max: 2, step: 0.1 },
      { key: "gear_floor", kind: "number", label: "Équipement de base sans récompenses", help: "Part d'un objet vert typique du niveau que le personnage porte de toute façon (marchands, objets trouvés).", unit: "%", scale: 0.01, min: 30, max: 120, step: 5 },
      { key: "gear_value", kind: "number", label: "Valeur d'une amélioration d'équipement", help: "XP que vaut un niveau de puissance gagné, en niveaux d'XP : favorise les quêtes à bonne récompense.", min: 0, max: 2, step: 0.1 },
      { key: "dungeon_loot_share", kind: "number", label: "Part du butin des donjons obtenue", help: "Chance de gagner un objet de boss qui vous sert, face aux autres joueurs du groupe.", unit: "%", scale: 0.01, min: 0, max: 100, step: 5 },
    ],
  },
  {
    title: "Entraînement de classe",
    fields: [
      { key: "train_min_gain", kind: "number", label: "Gain des sorts pour s'entraîner en passant", help: "L'entraîneur proche est visité quand les nouveaux sorts utiles rendent les combats au moins ce pourcentage plus rapides.", unit: "%", scale: 0.01, min: 0, max: 30, step: 1 },
      { key: "train_trip_gain", kind: "number", label: "Gain des sorts pour un trajet dédié", help: "Un trajet exprès chez l'entraîneur quand les sorts manquants ralentissent les combats de ce pourcentage.", unit: "%", scale: 0.01, min: 0, max: 60, step: 1 },
      { key: "train_every", kind: "number", label: "Nouveaux sorts tous les", help: "Sans données de sorts (TBC).", unit: "niveaux", min: 1, max: 10, step: 1 },
      { key: "train_radius", kind: "number", label: "Entraîneur pris au passage s'il est à moins de", help: "Un entraîneur de classe plus proche que cette distance de la route est visité en passant.", unit: "m", min: 0, max: 2000, step: 50 },
      { key: "train_max_delay", kind: "number", label: "Retard toléré avant un trajet dédié", help: "Niveaux de retard d'entraînement acceptés avant un trajet exprès chez l'entraîneur.", unit: "niveaux", min: 0, max: 10, step: 1 },
      { key: "train_time", kind: "number", label: "Temps chez l'entraîneur", help: "Temps passé chez l'entraîneur à apprendre les sorts.", unit: "s", min: 0, max: 120, step: 5 },
    ],
  },
  {
    title: "Sacs",
    fields: [
      { key: "bag_weight", kind: "number", label: "Coût d'un objet perdu, sacs pleins", help: "Temps que coûte chaque objet laissé faute de place dans les sacs (0 = les sacs ne comptent pas) : le guide passe par les vendeurs pour garder de la place.", unit: "s", min: 0, max: 120, step: 5 },
      { key: "loot_per_kill", kind: "number", label: "Place prise par le butin d'un mob", help: "Cases de sac que prend en moyenne le butin d'un mob tué, jusqu'à la vente. Les sacs grandissent avec le niveau, les objets de quête y prennent aussi leur place.", min: 0, max: 2, step: 0.05 },
      { key: "sell_radius", kind: "number", label: "Vendeur pris au passage s'il est à moins de", help: "Un vendeur plus proche que cette distance de la route est visité quand le butin remplit la moitié de la place.", unit: "m", min: 0, max: 1000, step: 25 },
      { key: "sell_time", kind: "number", label: "Temps chez le vendeur", help: "Temps passé à vendre le butin.", unit: "s", min: 0, max: 120, step: 5 },
      { key: "gather_share", kind: "number", label: "Place gardée par métier de récolte", help: "Part des sacs que gardent les herbes, minerais ou peaux de chaque métier de récolte (ou le sac de métier qui les contient), à partir du niveau où il est pris.", unit: "%", scale: 0.01, min: 0, max: 50, step: 5 },
    ],
  },
  {
    title: "Modèle",
    expert: true,
    help: "Valeurs utilisées quand les données du jeu ne disent rien.",
    fields: [
      { key: "dungeon_xp_share", kind: "number", label: "Part d'XP des mobs en groupe de 5", help: "Part de l'XP solo des mobs d'un donjon (élites compris) gagnée en groupe de 5.", unit: "%", scale: 0.01, min: 10, max: 100, step: 5 },
      { key: "explore_radius", kind: "number", label: "Distance de découverte d'un lieu", help: "XP d'exploration : un lieu est découvert en passant à cette distance de son centre.", unit: "m", min: 50, max: 800, step: 25 },
      { key: "default_xp_difficulty", kind: "number", label: "Colonne QuestXP par défaut", help: "Colonne de la table QuestXP du jeu utilisée quand l'XP d'une quête n'est pas connue : plus haut, plus d'XP.", min: 0, max: 9, step: 1 },
      { key: "common_item_sources", kind: "number", label: "Objet courant à partir de N types de mobs", help: "Tissus, gemmes, potions… lâchés par au moins ce nombre de types de mobs : ramassés en route ou achetés, pas farmés.", min: 10, max: 1000, step: 10 },
      { key: "common_item_time", kind: "number", label: "Temps par objet courant", help: "Temps compté par unité d'un objet courant (ramassé en route ou acheté à l'hôtel des ventes).", unit: "s", min: 0, max: 300, step: 10 },
      { key: "max_object_loot", kind: "number", label: "Ignorer les coffres au trésor", help: "Les objets du monde qui donnent plus de N butins différents (coffres au trésor) ne comptent pas comme source d'un objet de quête trouvable ailleurs. 0 = tous comptent.", min: 0, max: 200, step: 5 },
      { key: "mandatory_penalty", kind: "number", label: "Pénalité par quête obligatoire manquée", help: "Temps compté par l'optimiseur pour chaque quête de classe obligatoire laissée de côté : plus haut, elles restent dans le guide coûte que coûte.", unit: "min", scale: 60, min: 0, max: 600, step: 10 },
    ],
  },
  {
    title: "Plan de construction",
    expert: true,
    help: "La route de départ de chaque recherche, avant l'optimisation. Aléatoire (recommandé) : chaque candidat de la course tire ces réglages au hasard (le premier garde vos valeurs) ; sinon vos valeurs sont imposées à toutes les recherches.",
    fields: [
      { key: "construction_random", kind: "bool", label: "Plan aléatoire", help: "Coché : réglages tirés au hasard pour chaque candidat, les meilleures routes sont gardées. Décoché : les valeurs ci-dessous sont forcées." },
      {
        key: "construction",
        kind: "select",
        label: "Construction",
        help: "Façon de construire la première route, avant de l'optimiser.",
        options: [
          { value: "greedy", label: "Plus proche voisin (recommandé)" },
          { value: "regions", label: "Par groupes de quêtes (expérimental)" },
        ],
      },
      { key: "zone_stay", kind: "number", label: "Finir la zone avant de partir", help: "Poids des étapes hors de la zone courante (1 = la plus proche d'abord). 4 (défaut) vide la zone avant d'en partir. Aléatoire : de 3.5 à 5.", min: 1, max: 5, step: 0.5 },
      { key: "continent_stay", kind: "number", label: "Rester sur le continent", help: "Poids des étapes sur l'autre continent (1 = aucune préférence). Plus haut : évite les allers-retours entre continents, voire un détour entier. Aléatoire : de 1 à 4.", min: 1, max: 6, step: 0.5 },
      { key: "dungeon_quest_weight", kind: "number", label: "Attrait des quêtes de donjon", help: "Poids des quêtes d'un donjon proche de votre niveau (plus bas = le donjon est prévu plus volontiers ; les donjons forcés utilisent 0.25). Aléatoire : de 0.1 à 0.6.", min: 0.1, max: 2, step: 0.05 },
      { key: "fit_penalty", kind: "number", label: "Pénalité des quêtes trop basses", help: "Par niveau sous le vôtre (au-delà de 2) : ces quêtes sont prises moins volontiers. Aléatoire : de 0 à 0.15.", min: 0, max: 2, step: 0.1 },
      { key: "fit_above_penalty", kind: "number", label: "Pénalité des quêtes trop hautes", help: "Par niveau au-dessus du vôtre. Aléatoire : de 0.2 à 0.8.", min: 0, max: 2, step: 0.1 },
      { key: "efficiency_power", kind: "number", label: "Préférence pour les quêtes rentables", help: "0 = seule la distance compte ; plus haut = on va plus loin pour une quête qui rapporte beaucoup. Aléatoire : de 0 à 0.2.", min: 0, max: 2, step: 0.1 },
    ],
  },
  {
    title: "Algorithme",
    expert: true,
    help: "Pour tester l'optimiseur : les valeurs par défaut sont les meilleures mesurées.",
    fields: [
      { key: "allow_grind", kind: "bool", label: "Farm autorisé", help: "Sans farm, le guide va chercher plus de quêtes, quitte à être plus long. Il ne farme que s'il ne reste aucune quête." },
      { key: "grind_weight", kind: "number", label: "Poids du farm", help: "1 = seul le temps total compte ; 1.5 (défaut) = presque pas de farm et guide le plus rapide mesuré ; plus haut = encore moins de farm, mais accepte des quêtes peu rentables (3 rallongeait le guide de 5 à 9h).", min: 1, max: 10, step: 0.5 },
      { key: "grind_cap", kind: "number", label: "Part maximale de farm par niveau", help: "XP de chaque niveau que le farm pur peut donner (100 % = sans limite). Au-delà, le guide préfère des quêtes, même moins rentables.", unit: "%", scale: 0.01, min: 0, max: 100, step: 5 },
      { key: "farm_cap", kind: "number", label: "Part maximale de farm en chemin par niveau", help: "XP de chaque niveau que les mobs tués en chemin peuvent donner, à part du farm pur (100 % = sans limite). Une fois atteinte, le guide ne tue plus les mobs croisés jusqu'au niveau suivant.", unit: "%", scale: 0.01, min: 0, max: 100, step: 5 },
      { key: "grind_over_weight", kind: "number", label: "Poids du farm au-delà de cette part", help: "Comme le poids du farm, pour le farm qui dépasse la part maximale : 4 (défaut) = une quête jusqu'à 4 fois moins rentable que le farm est préférée.", min: 1, max: 10, step: 0.5 },
      { key: "alns", kind: "number", label: "Part de grands remaniements", help: "Part des essais qui retirent un morceau du guide (zone, tranche de niveaux, quêtes rendues trop tard) puis le reconstruisent. 0 = désactivé.", unit: "%", scale: 0.01, min: 0, max: 80, step: 5 },
      { key: "candidates", kind: "number", label: "Candidats de la course", help: "Routes construites (chacune sa graine et son plan) et brièvement optimisées ; les meilleures continuent, une recherche chacune. Égal au nombre de recherches = pas de course. Moins nombreux en aperçu.", min: 1, max: 256, step: 1 },
      { key: "race_mutants", kind: "number", label: "Mutants de la course", help: "Candidats construits en plus, comme les meilleurs des premiers avec un changement de zone fait autrement (partir plus tôt, plus tard, ou ailleurs). Moins nombreux en aperçu. 0 = aucun.", min: 0, max: 1024, step: 8 },
      { key: "screen", kind: "number", label: "Candidats optimisés", help: "Seuls ce nombre de candidats, les meilleurs une fois construits, sont optimisés dans la course : le classement des routes construites désigne déjà le gagnant, et construire coûte peu. 0 = tous.", min: 0, max: 256, step: 1 },
      { key: "qualify_tries", kind: "number", label: "Essais de la course", help: "Essais de chaque candidat, par étape de sa route (multipliés par l'effort de calcul).", min: 0, max: 50, step: 0.5 },
      { key: "qualify_min_tries", kind: "number", label: "Essais minimum de la course", help: "Au moins ce nombre d'essais par candidat (multiplié par l'effort de calcul) : sur un guide court, quelques essais par étape laissent les routes au début de leur amélioration, où la course les classe au hasard.", min: 0, max: 100000, step: 1000 },
      { key: "round_tries", kind: "number", label: "Essais par manche", help: "Essais de chaque recherche dans une manche, par étape de sa route. Après chaque manche, la meilleure moitié des routes continue, avec deux fois plus de recherches chacune. 0 = pas de manche : toutes les recherches améliorent le meilleur départ.", min: 0, max: 500, step: 5 },
      { key: "final_routes", kind: "number", label: "Routes en finale", help: "Les manches s'arrêtent à ce nombre de routes : la finale, dont la meilleure route donne le guide.", min: 1, max: 32, step: 1 },
      { key: "final_tries", kind: "number", label: "Essais de la finale", help: "Essais de chaque recherche de la finale, par étape de sa route.", min: 0, max: 2000, step: 10 },
      { key: "share_every", kind: "number", label: "Partage entre recherches", help: "Tous les combien d'essais les recherches d'une même route mettent en commun leur meilleur guide : celles qui sont derrière repartent du meilleur. 0 = chacune seule.", min: 0, max: 20000, step: 100 },
      { key: "threads", kind: "number", label: "Recherches en parallèle", help: "Nombre de recherches, pas de processeurs : le guide est le même sur tout PC, le calcul seulement plus long avec moins de cœurs.", min: 1, max: 32, step: 1 },
      { key: "seed", kind: "number", label: "Graine aléatoire", help: "Point de départ des tirages au hasard : la même graine redonne le même guide, une autre en explore d'autres.", min: 0, step: 1 },
    ],
  },
];

/** Settings of the simple view (left out of the advanced sections). */
export const SIMPLE_KEYS: (keyof Params)[] = ["dungeons", "allow_elite", "crowded", "progression", "dungeon_from", "dungeon_to", "death_skips"];

/** Speed presets applied on top of the defaults. */
export const presets: { label: string; values: Partial<Params> }[] = [
  { label: "Détendu", values: { kill_time: 33, grind_kill_time: 66, stop_overhead: 12, object_time: 15 } },
  { label: "Normal", values: { kill_time: 24, grind_kill_time: 53, stop_overhead: 8, object_time: 12 } },
  { label: "Rapide", values: { kill_time: 17, grind_kill_time: 40, stop_overhead: 5, object_time: 8 } },
];

/** Calculation presets: how much the optimization tries (its time depends on the processor). */
export const efforts: { label: string; effort: number }[] = [
  { label: "Aperçu", effort: 0.1 },
  { label: "Normal", effort: 1 },
  { label: "Poussé", effort: 3 },
];

/** Below this starting level there is no choice of start to offer (the engine agrees). */
export const MIN_CHOICE_LEVEL = 10;
