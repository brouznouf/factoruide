// Description of every planner setting, used to generate the settings form.
import type { Params } from "./api";

export type Field =
  | { key: keyof Params; kind: "number"; label: string; help?: string; unit?: string; min?: number; max?: number; step?: number; scale?: number }
  | { key: keyof Params; kind: "bool"; label: string; help?: string }
  | { key: keyof Params; kind: "select"; label: string; help?: string; options: { value: string; label: string }[] };

export interface Section {
  title: string;
  help?: string;
  fields: Field[];
}

export const sections: Section[] = [
  {
    title: "Contenu",
    fields: [
      { key: "dungeons", kind: "bool", label: "Faire les donjons rentables en groupe", help: "Un donjon est fait, avec ses quêtes, quand il fait gagner du temps." },
      { key: "dungeon_pickup_ahead", kind: "number", label: "Prendre les quêtes de donjon jusqu'à N niveaux avant", help: "Prises en passant à côté du donneur, gardées jusqu'au donjon.", unit: "niveaux", min: 0, max: 20, step: 1 },
      { key: "allow_elite", kind: "bool", label: "Quêtes élites (groupe)", help: "Quêtes dont les objectifs sont des mobs élites hors donjon." },
      { key: "solo_elite_kills", kind: "number", label: "Élites seules (en solo)", help: "Sans les quêtes élites, garde celles qui demandent au plus ce nombre de kills d'élite (Hogger, Tharil'zun, Mor'Ladim...). 0 = aucune.", min: 0, max: 5, step: 1 },
      { key: "elite_kill_factor", kind: "number", label: "Durée d'un kill d'élite", help: "Par rapport à un mob normal du même niveau (les élites donnent 2× l'XP).", min: 1, max: 6, step: 0.5 },
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
      { key: "elite_pull_levels", kind: "number", label: "Niveaux demandés par élite en plus dans un camp", help: "1 : un camp où l'on tire trois élites à la fois demande deux niveaux de plus qu'un élite isolé.", min: 0, max: 4, step: 0.5 },
      { key: "max_quest_above", kind: "number", label: "Quêtes au-dessus du niveau visé prises en compte", help: "Jusqu'à N niveaux au-dessus du niveau visé : prises en route, rendues avant si c'est rentable.", min: 0, max: 60, step: 1 },
      { key: "pvp_quests", kind: "bool", label: "Quêtes de champ de bataille (PvP)", help: "Marques d'honneur et objectifs de champ de bataille, comptés à la durée des parties." },
      { key: "pvp_mark_time", kind: "number", label: "Temps de champ de bataille par marque ou objectif", unit: "min", scale: 60, min: 5, max: 60, step: 5 },
      { key: "min_efficiency", kind: "number", label: "Rentabilité minimum d'une quête", help: "Par rapport au farm : 0.5 = une quête doit rapporter au moins la moitié de l'XP/heure du farm.", min: 0, max: 3, step: 0.1 },
      { key: "quest_log_size", kind: "number", label: "Taille du journal de quêtes", min: 5, max: 40, step: 1 },
    ],
  },
  {
    title: "Groupe",
    help: "Quand vous montez à plusieurs (joueurs choisis dans l'écran Essentiel).",
    fields: [
      { key: "group_kill_speed", kind: "number", label: "Dégâts de chaque joueur en plus", help: "Part des dégâts d'un joueur seul : 0.8 = un duo tue 1,8 fois plus vite.", min: 0.2, max: 1, step: 0.05 },
      { key: "group_shared_drops", kind: "bool", label: "Objets de quête pour tout le groupe", help: "Un objet de quête qui tombe est ramassé par chaque joueur qui a la quête. Sinon chacun doit avoir le sien (plus de mobs à tuer)." },
    ],
  },
  {
    title: "Rythme de jeu",
    help: "Ajustez à votre façon de jouer : un joueur rapide tue plus vite et se repose moins.",
    fields: [
      { key: "kill_time", kind: "number", label: "Temps par mob de quête", unit: "s", min: 5, max: 60, step: 1, help: "Recherche et repos compris, mob de votre niveau, avec un équipement de base et tous les sorts." },
      { key: "grind_kill_time", kind: "number", label: "Temps par mob en farm", unit: "s", min: 10, max: 90, step: 1 },
      { key: "object_time", kind: "number", label: "Temps par objet à ramasser", unit: "s", min: 2, max: 60, step: 1 },
      { key: "gather_overlap", kind: "number", label: "Récolte entre deux kills", help: "Part du temps d'un kill (repos, recherche du mob suivant) pendant laquelle on ramasse les objets de quête autour : une quête de récolte au milieu des mobs d'une autre quête se fait en même temps. 0 = l'une après l'autre.", unit: "%", scale: 0.01, min: 0, max: 80, step: 5 },
      { key: "gather_radius", kind: "number", label: "Récolte et kills regroupés jusqu'à", help: "Distance entre une zone de récolte et une zone de mobs de quêtes en cours pour les faire ensemble.", unit: "m", min: 0, max: 400, step: 25 },
      { key: "stop_overhead", kind: "number", label: "Temps par étape (dialogue, loot)", unit: "s", min: 0, max: 60, step: 1 },
      { key: "default_drop_chance", kind: "number", label: "Taux de drop par défaut", help: "Quand le vrai taux n'est pas connu.", min: 0.05, max: 1, step: 0.05 },
      { key: "min_start_chance", kind: "number", label: "Drop minimum pour démarrer une quête", help: "Les quêtes démarrées par un objet plus rare ne sont pas planifiées : on les prend s'il tombe.", min: 0, max: 0.5, step: 0.01 },
      { key: "default_kill_count", kind: "number", label: "Mobs à tuer par défaut", min: 1, max: 30, step: 1 },
      { key: "default_item_count", kind: "number", label: "Objets à ramasser par défaut", min: 1, max: 30, step: 1 },
    ],
  },
  {
    title: "Déplacements",
    fields: [
      { key: "run_speed", kind: "number", label: "Vitesse de course", unit: "m/s", min: 5, max: 10, step: 0.1 },
      { key: "mount_level", kind: "number", label: "Niveau de la monture", min: 1, max: 60, step: 1 },
      { key: "mount_bonus", kind: "number", label: "Bonus de vitesse de la monture", help: "0.6 = +60 %", min: 0, max: 1, step: 0.05 },
      { key: "detour", kind: "number", label: "Facteur de détour", help: "Les chemins sont plus longs que la ligne droite.", min: 1, max: 2, step: 0.05 },
      { key: "shortcuts", kind: "bool", label: "Raccourcis hors route", help: "Escalades et sauts connus (ex. Dun Morogh vers les Paluns par la montagne), déclarés dans overrides/passes.toml." },
      { key: "flight_speed", kind: "number", label: "Vitesse des vols", unit: "m/s", min: 10, max: 60, step: 1 },
      { key: "flight_overhead", kind: "number", label: "Temps fixe par vol", unit: "s", min: 0, max: 120, step: 5 },
      { key: "camp_radius", kind: "number", label: "Quêtes prises et rendues au passage jusqu'à", help: "À chaque étape, les quêtes du guide dont le PNJ est à cette distance sont rendues et prises tout de suite, comme dans un camp (0 = seulement à leur propre étape).", unit: "m", min: 0, max: 300, step: 10 },
      { key: "along_corridor", kind: "number", label: "Mobs tués en chemin jusqu'à", help: "Les mobs des quêtes en cours croisés à cette distance du trajet vers l'étape suivante sont tués au passage (0 = jamais).", unit: "m", min: 0, max: 150, step: 10 },
      { key: "farm_on_way", kind: "bool", label: "Farmer en chemin", help: "À pied, les mobs normaux croisés sont tués au passage, comme le demandent la plupart des guides. Ce farm remplace le farm pur : il compte dans la part maximale de farm par niveau et s'arrête une fois celle-ci atteinte." },
      { key: "farm_corridor", kind: "number", label: "Mobs farmés en chemin jusqu'à", help: "Distance au trajet des mobs tués en chemin.", unit: "m", min: 5, max: 80, step: 5 },
      { key: "farm_below", kind: "number", label: "Farm en chemin : niveaux en dessous", help: "Les mobs plus bas que votre niveau moins N rapportent trop peu : ignorés.", unit: "niveaux", min: 0, max: 10, step: 1 },
      { key: "farm_above", kind: "number", label: "Farm en chemin : niveaux au-dessus", help: "Les mobs plus hauts que votre niveau plus N sont trop longs ou dangereux : contournés.", unit: "niveaux", min: 0, max: 5, step: 1 },
      { key: "flight_learn_radius", kind: "number", label: "Détour max pour apprendre un point de vol", unit: "m", min: 0, max: 1500, step: 50 },
      { key: "hearth_cooldown", kind: "number", label: "Recharge de la pierre de foyer", unit: "min", scale: 60, min: 0, max: 60, step: 5 },
      { key: "hearth_cast", kind: "number", label: "Incantation de la pierre", unit: "s", min: 0, max: 20, step: 1 },
      { key: "gold_weight", kind: "number", label: "Économiser l'or des vols", help: "Temps que vaut une pièce d'or dépensée en vols (0 = vols gratuits). 60 min (défaut) divise par 2 l'or dépensé et regroupe les quêtes par zone sans rallonger la route.", unit: "min", scale: 60, min: 0, max: 240, step: 10 },
      { key: "zone_change_weight", kind: "number", label: "Coût d'un changement de zone", help: "Temps compté en plus du trajet à chaque changement de zone : regroupe les quêtes par zone au lieu d'aller chercher quelques quêtes loin. 4 min par défaut.", unit: "min", scale: 60, min: 0, max: 15, step: 1 },
      { key: "flight_weight", kind: "number", label: "Poids des vols", help: "1 (défaut) = seul leur temps compte ; plus haut = marche ou réordonne la route plutôt que prendre un vol qui fait gagner peu de temps.", min: 1, max: 5, step: 0.25 },
    ],
  },
  {
    title: "Puissance du personnage",
    help: "La difficulté des combats suit la puissance (niveau + équipement + sorts appris) et l'XP suit le niveau : les quêtes qui donnent du bon équipement et les donjons rendent la suite plus rapide.",
    fields: [
      { key: "power_scale", kind: "number", label: "Poids de l'équipement et des sorts", help: "1 = le modèle tel quel ; 0 = la puissance est le niveau.", min: 0, max: 2, step: 0.1 },
      { key: "gear_floor", kind: "number", label: "Équipement de base sans récompenses", help: "Part d'un objet vert typique du niveau que le personnage porte de toute façon (marchands, objets trouvés).", min: 0.3, max: 1.2, step: 0.05 },
      { key: "gear_value", kind: "number", label: "Valeur d'une amélioration d'équipement", help: "XP que vaut un niveau de puissance gagné, en niveaux d'XP : favorise les quêtes à bonne récompense.", min: 0, max: 2, step: 0.1 },
      { key: "dungeon_loot_share", kind: "number", label: "Part du butin des donjons obtenue", help: "Chance de gagner un objet de boss qui vous sert, face aux autres joueurs du groupe.", min: 0, max: 1, step: 0.05 },
    ],
  },
  {
    title: "Entraînement de classe",
    fields: [
      { key: "train_min_gain", kind: "number", label: "Gain des sorts pour s'entraîner en passant", help: "L'entraîneur proche est visité quand les nouveaux sorts utiles rendent les combats au moins ce pourcentage plus rapides.", unit: "%", scale: 0.01, min: 0, max: 30, step: 1 },
      { key: "train_trip_gain", kind: "number", label: "Gain des sorts pour un trajet dédié", help: "Un trajet exprès chez l'entraîneur quand les sorts manquants ralentissent les combats de ce pourcentage.", unit: "%", scale: 0.01, min: 0, max: 60, step: 1 },
      { key: "train_every", kind: "number", label: "Nouveaux sorts tous les", help: "Sans données de sorts (TBC).", unit: "niveaux", min: 1, max: 10, step: 1 },
      { key: "train_radius", kind: "number", label: "Entraîneur pris au passage s'il est à moins de", unit: "m", min: 0, max: 2000, step: 50 },
      { key: "train_max_delay", kind: "number", label: "Retard toléré avant un trajet dédié", unit: "niveaux", min: 0, max: 10, step: 1 },
      { key: "train_time", kind: "number", label: "Temps chez l'entraîneur", unit: "s", min: 0, max: 120, step: 5 },
    ],
  },
  {
    title: "Modèle",
    help: "Valeurs utilisées quand les données du jeu ne disent rien.",
    fields: [
      { key: "dungeon_xp_share", kind: "number", label: "Part d'XP des mobs en groupe de 5", min: 0.1, max: 1, step: 0.05 },
      { key: "explore_radius", kind: "number", label: "Distance de découverte d'un lieu", help: "XP d'exploration : un lieu est découvert en passant à cette distance de son centre.", unit: "m", min: 50, max: 800, step: 25 },
      { key: "default_xp_difficulty", kind: "number", label: "Colonne QuestXP par défaut", min: 0, max: 9, step: 1 },
      { key: "common_item_sources", kind: "number", label: "Objet courant à partir de N types de mobs", help: "Tissus, gemmes, potions… lâchés par au moins ce nombre de types de mobs : ramassés en route ou achetés, pas farmés.", min: 10, max: 1000, step: 10 },
      { key: "common_item_time", kind: "number", label: "Temps par objet courant", help: "Temps compté par unité d'un objet courant (ramassé en route ou acheté à l'hôtel des ventes).", unit: "s", min: 0, max: 300, step: 10 },
      { key: "max_object_loot", kind: "number", label: "Ignorer les coffres au trésor", help: "Les objets du monde qui donnent plus de N butins différents (coffres au trésor) ne comptent pas comme source d'un objet de quête trouvable ailleurs. 0 = tous comptent.", min: 0, max: 200, step: 5 },
      { key: "mandatory_penalty", kind: "number", label: "Pénalité par quête obligatoire manquée", unit: "min", scale: 60, min: 0, max: 600, step: 10 },
    ],
  },
  {
    title: "Plan de construction",
    help: "La route de départ de chaque recherche, avant l'optimisation. Aléatoire (recommandé) : chaque candidat de la course tire ces réglages au hasard (le premier garde vos valeurs) ; sinon vos valeurs sont imposées à toutes les recherches.",
    fields: [
      { key: "construction_random", kind: "bool", label: "Plan aléatoire", help: "Coché : réglages tirés au hasard pour chaque candidat, les meilleures routes sont gardées. Décoché : les valeurs ci-dessous sont forcées." },
      {
        key: "construction",
        kind: "select",
        label: "Construction",
        options: [
          { value: "greedy", label: "Plus proche voisin (recommandé)" },
          { value: "regions", label: "Par groupes de quêtes (expérimental)" },
        ],
      },
      { key: "zone_stay", kind: "number", label: "Finir la zone avant de partir", help: "Poids des étapes hors de la zone courante (1 = la plus proche d'abord). 2.5 (défaut) vide la zone avant d'en partir. Aléatoire : de 1.5 à 3.5.", min: 1, max: 5, step: 0.5 },
      { key: "continent_stay", kind: "number", label: "Rester sur le continent", help: "Poids des étapes sur l'autre continent (1 = aucune préférence). Plus haut : évite les allers-retours entre continents, voire un détour entier. Aléatoire : de 1 à 4.", min: 1, max: 6, step: 0.5 },
      { key: "dungeon_quest_weight", kind: "number", label: "Attrait des quêtes de donjon", help: "Poids des quêtes d'un donjon proche de votre niveau (plus bas = le donjon est prévu plus volontiers ; les donjons forcés utilisent 0.25). Aléatoire : de 0.25 à 1.", min: 0.1, max: 2, step: 0.05 },
      { key: "fit_penalty", kind: "number", label: "Pénalité des quêtes trop basses", help: "Par niveau sous le vôtre (au-delà de 2) : ces quêtes sont prises moins volontiers. Aléatoire : de 0 à 0.4.", min: 0, max: 2, step: 0.1 },
      { key: "fit_above_penalty", kind: "number", label: "Pénalité des quêtes trop hautes", help: "Par niveau au-dessus du vôtre. Aléatoire : de 0 à 0.6.", min: 0, max: 2, step: 0.1 },
      { key: "efficiency_power", kind: "number", label: "Préférence pour les quêtes rentables", help: "0 = seule la distance compte ; plus haut = on va plus loin pour une quête qui rapporte beaucoup. Aléatoire : de 0 à 0.6.", min: 0, max: 2, step: 0.1 },
    ],
  },
  {
    title: "Algorithme",
    help: "Pour tester l'optimiseur : les valeurs par défaut sont les meilleures mesurées.",
    fields: [
      { key: "time_limit_ms", kind: "number", label: "Temps de calcul", help: "Temps total : une course entre des départs candidats, puis l'amélioration des meilleurs.", unit: "s", scale: 1000, min: 1, max: 900, step: 1 },
      { key: "allow_grind", kind: "bool", label: "Farm autorisé", help: "Sans farm, le guide va chercher plus de quêtes, quitte à être plus long. Il ne farme que s'il ne reste aucune quête." },
      { key: "grind_weight", kind: "number", label: "Poids du farm", help: "1 = seul le temps total compte ; 1.5 (défaut) = presque pas de farm et guide le plus rapide mesuré ; plus haut = encore moins de farm, mais accepte des quêtes peu rentables (3 rallongeait le guide de 5 à 9h).", min: 1, max: 10, step: 0.5 },
      { key: "grind_cap", kind: "number", label: "Part maximale de farm par niveau", help: "XP de chaque niveau que le farm peut donner, farm en chemin compris (100 % = sans limite). Au-delà, le guide préfère des quêtes, même moins rentables.", unit: "%", scale: 0.01, min: 0, max: 100, step: 5 },
      { key: "grind_over_weight", kind: "number", label: "Poids du farm au-delà de cette part", help: "Comme le poids du farm, pour le farm qui dépasse la part maximale : 4 (défaut) = une quête jusqu'à 4 fois moins rentable que le farm est préférée.", min: 1, max: 10, step: 0.5 },
      { key: "alns", kind: "number", label: "Part de grands remaniements", help: "Part des essais qui retirent un morceau du guide (zone, tranche de niveaux, quêtes rendues trop tard) puis le reconstruisent. 0 = désactivé.", min: 0, max: 0.8, step: 0.05 },
      { key: "candidates", kind: "number", label: "Candidats de la course", help: "Routes construites (chacune sa graine et son plan) et brièvement optimisées ; les meilleures continuent. Égal au nombre de recherches = pas de course.", min: 1, max: 128, step: 1 },
      { key: "qualify_share", kind: "number", label: "Part de la course entre départs", help: "Part du temps de calcul donnée à la course entre départs candidats, le reste améliore les meilleurs. 0 = pas de course.", min: 0, max: 0.6, step: 0.05 },
      { key: "threads", kind: "number", label: "Recherches en parallèle", min: 1, max: 32, step: 1 },
      { key: "seed", kind: "number", label: "Graine aléatoire", min: 0, step: 1 },
    ],
  },
];

/** Settings of the simple view (left out of the advanced sections). */
export const SIMPLE_KEYS: (keyof Params)[] = ["dungeons", "allow_elite"];

/** Speed presets applied on top of the defaults. */
export const presets: { label: string; values: Partial<Params> }[] = [
  { label: "Détendu", values: { kill_time: 33, grind_kill_time: 66, stop_overhead: 12, object_time: 15 } },
  { label: "Normal", values: { kill_time: 24, grind_kill_time: 53, stop_overhead: 8, object_time: 12 } },
  { label: "Rapide", values: { kill_time: 17, grind_kill_time: 40, stop_overhead: 5, object_time: 8 } },
];

/** Calculation presets: the whole optimization time. */
export const budgets: { label: string; ms: number }[] = [
  { label: "Aperçu (10 s)", ms: 10_000 },
  { label: "Normal (2 min)", ms: 120_000 },
  { label: "Poussé (6 min)", ms: 360_000 },
];
