//! The sentences of the guide in its 10 languages: a template per key, with `{name}` for its
//! arguments (see `Phrase`). Mexican Spanish uses Spain's.

/// A language of the guide's sentences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Fr,
    De,
    Es,
    Pt,
    Ru,
    Ko,
    ZhCn,
    ZhTw,
}

impl Lang {
    /// The language of a game locale (enUS, frFR...); English when unknown.
    pub fn of(locale: &str) -> Self {
        match locale {
            "frFR" => Self::Fr,
            "deDE" => Self::De,
            "esES" | "esMX" => Self::Es,
            "ptBR" => Self::Pt,
            "ruRU" => Self::Ru,
            "koKR" => Self::Ko,
            "zhCN" => Self::ZhCn,
            "zhTW" => Self::ZhTw,
            _ => Self::En,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Templates by key: English, French, German, Spanish, Portuguese, Russian, Korean, simplified
/// and traditional Chinese.
pub(super) const TEMPLATES: &[(&str, [&str; 9])] = &[
    (
        "accept",
        [
            "Accept {quest} from {who}",
            "Prendre {quest} auprès de {who}",
            "{quest} bei {who} annehmen",
            "Aceptar {quest} de {who}",
            "Aceitar {quest} de {who}",
            "Взять задание «{quest}» ({who})",
            "{who}에게서 {quest} 수락",
            "从{who}处接受{quest}",
            "從{who}接受{quest}",
        ],
    ),
    (
        "accept.item",
        [
            "Loot {who} to start {quest}",
            "Looter {who} pour démarrer {quest}",
            "{who} looten, um {quest} zu beginnen",
            "Despojar {who} para empezar {quest}",
            "Saquear {who} para iniciar {quest}",
            "Добыть {who}, чтобы начать «{quest}»",
            "{who} 획득하여 {quest} 시작",
            "拾取{who}以开始{quest}",
            "拾取{who}以開始{quest}",
        ],
    ),
    (
        "turnin",
        [
            "Turn in {quest} to {who}",
            "Rendre {quest} à {who}",
            "{quest} bei {who} abgeben",
            "Entregar {quest} a {who}",
            "Entregar {quest} a {who}",
            "Сдать «{quest}» ({who})",
            "{who}에게 {quest} 완료",
            "向{who}交付{quest}",
            "向{who}交付{quest}",
        ],
    ),
    (
        "objective.kill",
        [
            "Kill {who}",
            "Tuer {who}",
            "{who} töten",
            "Matar a {who}",
            "Matar {who}",
            "Убить: {who}",
            "{who} 처치",
            "击杀{who}",
            "擊殺{who}",
        ],
    ),
    (
        "objective.talk",
        [
            "Talk to {who}",
            "Parler à {who}",
            "Mit {who} sprechen",
            "Hablar con {who}",
            "Falar com {who}",
            "Поговорить: {who}",
            "{who}와 대화",
            "与{who}交谈",
            "與{who}交談",
        ],
    ),
    (
        "objective.use",
        [
            "Use {who}",
            "Utiliser {who}",
            "{who} benutzen",
            "Usar {who}",
            "Usar {who}",
            "Использовать: {who}",
            "{who} 사용",
            "使用{who}",
            "使用{who}",
        ],
    ),
    (
        "objective.buy",
        [
            "Buy {who}",
            "Acheter {who}",
            "{who} kaufen",
            "Comprar {who}",
            "Comprar {who}",
            "Купить: {who}",
            "{who} 구입",
            "购买{who}",
            "購買{who}",
        ],
    ),
    (
        "objective.get",
        [
            "{who}", "{who}", "{who}", "{who}", "{who}", "{who}", "{who}", "{who}", "{who}",
        ],
    ),
    (
        "objective.text",
        [
            "{text}", "{text}", "{text}", "{text}", "{text}", "{text}", "{text}", "{text}", "{text}",
        ],
    ),
    (
        "text",
        [
            "{text}", "{text}", "{text}", "{text}", "{text}", "{text}", "{text}", "{text}", "{text}",
        ],
    ),
    (
        "suffix.optional",
        [
            "(optional: {profession} {skill})",
            "(facultatif : {profession} {skill})",
            "(optional: {profession} {skill})",
            "(opcional: {profession} {skill})",
            "(opcional: {profession} {skill})",
            "(необязательно: {profession} {skill})",
            "(선택: {profession} {skill})",
            "（可选：{profession} {skill}）",
            "（可選：{profession} {skill}）",
        ],
    ),
    (
        "suffix.reward",
        [
            "(take {item})",
            "(prendre {item})",
            "({item} wählen)",
            "(elegir {item})",
            "(pegar {item})",
            "(награда: {item})",
            "({item} 선택)",
            "（选择{item}）",
            "（選擇{item}）",
        ],
    ),
    (
        "suffix.farm",
        [
            "(kill ~{n} mobs on the way)",
            "(tuer ~{n} mobs en chemin)",
            "(~{n} Gegner unterwegs töten)",
            "(matar ~{n} enemigos por el camino)",
            "(matar ~{n} inimigos no caminho)",
            "(убить ~{n} врагов по пути)",
            "(가는 길에 몹 ~{n}마리 처치)",
            "（途中击杀约{n}个怪物）",
            "（途中擊殺約{n}隻怪物）",
        ],
    ),
    (
        "suffix.farm1",
        [
            "(kill ~{n} mob on the way)",
            "(tuer ~{n} mob en chemin)",
            "(~{n} Gegner unterwegs töten)",
            "(matar ~{n} enemigo por el camino)",
            "(matar ~{n} inimigo no caminho)",
            "(убить ~{n} врага по пути)",
            "(가는 길에 몹 ~{n}마리 처치)",
            "（途中击杀约{n}个怪物）",
            "（途中擊殺約{n}隻怪物）",
        ],
    ),
    (
        "abandon",
        [
            "Abandon {quest} (can no longer be finished)",
            "Abandonner {quest} (ne peut plus être finie)",
            "{quest} abbrechen (kann nicht mehr abgeschlossen werden)",
            "Abandonar {quest} (ya no se puede completar)",
            "Abandonar {quest} (não pode mais ser concluída)",
            "Отменить «{quest}» (её уже нельзя выполнить)",
            "{quest} 포기 (더 이상 완료할 수 없음)",
            "放弃{quest}（已无法完成）",
            "放棄{quest}（已無法完成）",
        ],
    ),
    (
        "abandon.unused",
        [
            "Abandon {quest} (not in this guide)",
            "Abandonner {quest} (pas dans ce guide)",
            "{quest} abbrechen (nicht in diesem Guide)",
            "Abandonar {quest} (no está en esta guía)",
            "Abandonar {quest} (não está neste guia)",
            "Отменить «{quest}» (нет в этом гайде)",
            "{quest} 포기 (이 가이드에 없음)",
            "放弃{quest}（不在本指南中）",
            "放棄{quest}（不在本指南中）",
        ],
    ),
    (
        "fly",
        [
            "Fly from {from} to {to}",
            "Vol de {from} à {to}",
            "Flug von {from} nach {to}",
            "Volar de {from} a {to}",
            "Voar de {from} para {to}",
            "Полёт: {from} → {to}",
            "{from}에서 {to}(으)로 비행",
            "从{from}飞往{to}",
            "從{from}飛往{to}",
        ],
    ),
    (
        "link",
        [
            "Take {link}",
            "Prendre : {link}",
            "{link} nehmen",
            "Tomar {link}",
            "Pegar {link}",
            "Сесть: {link}",
            "{link} 탑승",
            "乘坐{link}",
            "搭乘{link}",
        ],
    ),
    (
        "link.then",
        [
            "Take {link} (then {last})",
            "Prendre : {link} (puis {last})",
            "{link} nehmen (dann {last})",
            "Tomar {link} (luego {last})",
            "Pegar {link} (depois {last})",
            "Сесть: {link} (затем {last})",
            "{link} 탑승 (이후 {last})",
            "乘坐{link}（然后{last}）",
            "搭乘{link}（然後{last}）",
        ],
    ),
    (
        "flight_master",
        [
            "Get the flight path at {place}",
            "Point de vol : {place}",
            "Flugpunkt: {place}",
            "Ruta de vuelo: {place}",
            "Rota de voo: {place}",
            "Точка полёта: {place}",
            "비행 경로: {place}",
            "飞行点：{place}",
            "飛行點：{place}",
        ],
    ),
    (
        "grind",
        [
            "Grind to level {level} (~{minutes} min)",
            "Farmer jusqu'au niveau {level} (~{minutes} min)",
            "Bis Stufe {level} grinden (~{minutes} Min.)",
            "Farmear hasta el nivel {level} (~{minutes} min)",
            "Farmar até o nível {level} (~{minutes} min)",
            "Гриндить до {level} уровня (~{minutes} мин)",
            "레벨 {level}까지 사냥 (~{minutes}분)",
            "刷怪至{level}级（约{minutes}分钟）",
            "練怪至{level}級（約{minutes}分鐘）",
        ],
    ),
    (
        "grind.xp",
        [
            "Grind to level {level} and {p}% ({xp} xp) before going on (~{minutes} min)",
            "Farmer jusqu'au niveau {level} et {p} % ({xp} xp) avant de continuer (~{minutes} min)",
            "Bis Stufe {level} und {p} % ({xp} EP) grinden, bevor es weitergeht (~{minutes} Min.)",
            "Farmear hasta el nivel {level} y {p} % ({xp} PX) antes de seguir (~{minutes} min)",
            "Farmar até o nível {level} e {p}% ({xp} XP) antes de continuar (~{minutes} min)",
            "Гриндить до {level} уровня и {p}% ({xp} опыта), прежде чем идти дальше (~{minutes} мин)",
            "다음으로 가기 전에 레벨 {level}, {p}% ({xp} 경험치)까지 사냥 (~{minutes}분)",
            "继续前刷怪至{level}级{p}%（{xp}经验）（约{minutes}分钟）",
            "繼續前練怪至{level}級{p}%（{xp}經驗）（約{minutes}分鐘）",
        ],
    ),
    (
        "checkpoint",
        [
            "Checkpoint: be level {level} before going on (grind if needed)",
            "Palier : être niveau {level} avant de continuer (farmer si besoin)",
            "Etappe: Stufe {level} erreichen, bevor es weitergeht (falls nötig grinden)",
            "Etapa: llegar al nivel {level} antes de seguir (farmear si hace falta)",
            "Marco: estar no nível {level} antes de continuar (farmar se precisar)",
            "Рубеж: достичь {level} уровня, прежде чем идти дальше (гриндить при необходимости)",
            "단계: 계속하기 전에 레벨 {level} 달성 (필요하면 사냥)",
            "节点：继续前达到{level}级（必要时刷怪）",
            "節點：繼續前達到{level}級（必要時練怪）",
        ],
    ),
    (
        "checkpoint.xp",
        [
            "Level {checkpoint} checkpoint: be level {level} and {p}% ({xp} xp) before going on (grind here if needed)",
            "Palier niveau {checkpoint} : être niveau {level} et {p} % ({xp} xp) avant de continuer (farmer ici si besoin)",
            "Etappe Stufe {checkpoint}: Stufe {level} und {p} % ({xp} EP) erreichen, bevor es weitergeht (falls nötig hier grinden)",
            "Etapa nivel {checkpoint}: llegar al nivel {level} y {p} % ({xp} PX) antes de seguir (farmear aquí si hace falta)",
            "Marco nível {checkpoint}: estar no nível {level} e {p}% ({xp} XP) antes de continuar (farmar aqui se precisar)",
            "Рубеж {checkpoint} уровня: достичь {level} уровня и {p}% ({xp} опыта), прежде чем идти дальше (гриндить здесь при необходимости)",
            "레벨 {checkpoint} 단계: 계속하기 전에 레벨 {level}, {p}% ({xp} 경험치) 달성 (필요하면 여기서 사냥)",
            "{checkpoint}级节点：继续前达到{level}级{p}%（{xp}经验）（必要时在此刷怪）",
            "{checkpoint}級節點：繼續前達到{level}級{p}%（{xp}經驗）（必要時在此練怪）",
        ],
    ),
    (
        "death_skip",
        [
            "Die and respawn at the spirit healer ({zone})",
            "Mourir et revenir à l'ange de la mort ({zone})",
            "Sterben und beim Geistheiler wiederbeleben ({zone})",
            "Morir y resucitar con el ángel de la resurrección ({zone})",
            "Morrer e reviver no Curandeiro Espiritual ({zone})",
            "Умереть и воскреснуть у целителя душ ({zone})",
            "죽은 뒤 영혼의 치유사에게서 부활 ({zone})",
            "死亡后在灵魂医者处复活（{zone}）",
            "死亡後在靈魂醫者處復活（{zone}）",
        ],
    ),
    (
        "hearth",
        [
            "Hearthstone to {zone}",
            "Pierre de foyer vers {zone}",
            "Ruhestein nach {zone}",
            "Piedra de hogar a {zone}",
            "Pedra de Regresso para {zone}",
            "Камень возвращения: {zone}",
            "귀환석: {zone}",
            "炉石返回{zone}",
            "爐石返回{zone}",
        ],
    ),
    (
        "hearth.town",
        [
            "Hearthstone to {town} ({zone})",
            "Pierre de foyer vers {town} ({zone})",
            "Ruhestein nach {town} ({zone})",
            "Piedra de hogar a {town} ({zone})",
            "Pedra de Regresso para {town} ({zone})",
            "Камень возвращения: {town} ({zone})",
            "귀환석: {town} ({zone})",
            "炉石返回{town}（{zone}）",
            "爐石返回{town}（{zone}）",
        ],
    ),
    (
        "train",
        [
            "Train new spells with {who}",
            "Apprendre les sorts auprès de {who}",
            "Neue Zauber bei {who} lernen",
            "Aprender hechizos nuevos con {who}",
            "Aprender feitiços novos com {who}",
            "Изучить новые заклинания ({who})",
            "{who}에게서 새 주문 배우기",
            "向{who}学习新法术",
            "向{who}學習新法術",
        ],
    ),
    (
        "train.spells",
        [
            "Train with {who}: {spells}",
            "Apprendre auprès de {who} : {spells}",
            "Bei {who} lernen: {spells}",
            "Aprender con {who}: {spells}",
            "Aprender com {who}: {spells}",
            "Обучение ({who}): {spells}",
            "{who}에게서 배우기: {spells}",
            "向{who}学习：{spells}",
            "向{who}學習：{spells}",
        ],
    ),
    (
        "bind",
        [
            "Set your hearthstone at {who} ({zone})",
            "Lier la pierre de foyer auprès de {who} ({zone})",
            "Ruhestein bei {who} binden ({zone})",
            "Vincular la piedra de hogar con {who} ({zone})",
            "Vincular a Pedra de Regresso com {who} ({zone})",
            "Привязать камень возвращения ({who}, {zone})",
            "{who}에게 귀환석 귀속 ({zone})",
            "在{who}处绑定炉石（{zone}）",
            "在{who}處綁定爐石（{zone}）",
        ],
    ),
    (
        "dungeon",
        [
            "Dungeon: {dungeon} with a group (~{minutes} min) - quests: {quests}",
            "Donjon : {dungeon} en groupe (~{minutes} min) - quêtes : {quests}",
            "Dungeon: {dungeon} in der Gruppe (~{minutes} Min.) - Quests: {quests}",
            "Mazmorra: {dungeon} en grupo (~{minutes} min) - misiones: {quests}",
            "Masmorra: {dungeon} em grupo (~{minutes} min) - missões: {quests}",
            "Подземелье: {dungeon} в группе (~{minutes} мин) - задания: {quests}",
            "던전: {dungeon} 파티 (~{minutes}분) - 퀘스트: {quests}",
            "地下城：{dungeon}组队（约{minutes}分钟）- 任务：{quests}",
            "地城：{dungeon}組隊（約{minutes}分鐘）- 任務：{quests}",
        ],
    ),
];

/// Profession names in the same languages.
const PROFESSIONS: &[(&str, [&str; 9])] = &[
    (
        "alchemy",
        [
            "Alchemy",
            "Alchimie",
            "Alchemie",
            "Alquimia",
            "Alquimia",
            "Алхимия",
            "연금술",
            "炼金术",
            "鍊金術",
        ],
    ),
    (
        "blacksmithing",
        [
            "Blacksmithing",
            "Forge",
            "Schmiedekunst",
            "Herrería",
            "Ferraria",
            "Кузнечное дело",
            "대장기술",
            "锻造",
            "鍛造",
        ],
    ),
    (
        "enchanting",
        [
            "Enchanting",
            "Enchantement",
            "Verzauberkunst",
            "Encantamiento",
            "Encantamento",
            "Наложение чар",
            "마법부여",
            "附魔",
            "附魔",
        ],
    ),
    (
        "engineering",
        [
            "Engineering",
            "Ingénierie",
            "Ingenieurskunst",
            "Ingeniería",
            "Engenharia",
            "Инженерное дело",
            "기계공학",
            "工程学",
            "工程學",
        ],
    ),
    (
        "herbalism",
        [
            "Herbalism",
            "Herboristerie",
            "Kräuterkunde",
            "Herboristería",
            "Herborismo",
            "Травничество",
            "약초채집",
            "草药学",
            "草藥學",
        ],
    ),
    (
        "leatherworking",
        [
            "Leatherworking",
            "Travail du cuir",
            "Lederverarbeitung",
            "Peletería",
            "Couraria",
            "Кожевничество",
            "가죽세공",
            "制皮",
            "製皮",
        ],
    ),
    (
        "mining",
        [
            "Mining",
            "Minage",
            "Bergbau",
            "Minería",
            "Mineração",
            "Горное дело",
            "채광",
            "采矿",
            "採礦",
        ],
    ),
    (
        "skinning",
        [
            "Skinning",
            "Dépeçage",
            "Kürschnerei",
            "Desuello",
            "Esfolamento",
            "Снятие шкур",
            "무두질",
            "剥皮",
            "剝皮",
        ],
    ),
    (
        "tailoring",
        [
            "Tailoring",
            "Couture",
            "Schneiderei",
            "Sastrería",
            "Alfaiataria",
            "Портняжное дело",
            "재봉술",
            "裁缝",
            "裁縫",
        ],
    ),
    (
        "cooking",
        [
            "Cooking",
            "Cuisine",
            "Kochkunst",
            "Cocina",
            "Culinária",
            "Кулинария",
            "요리",
            "烹饪",
            "烹飪",
        ],
    ),
    (
        "fishing",
        [
            "Fishing",
            "Pêche",
            "Angeln",
            "Pesca",
            "Pesca",
            "Рыбная ловля",
            "낚시",
            "钓鱼",
            "釣魚",
        ],
    ),
    (
        "first_aid",
        [
            "First Aid",
            "Secourisme",
            "Erste Hilfe",
            "Primeros auxilios",
            "Primeiros Socorros",
            "Первая помощь",
            "응급치료",
            "急救",
            "急救",
        ],
    ),
];

/// The template of `key` in `lang` (English when the key is unknown to the language; the key
/// itself when unknown, so that a newer guide still shows something).
pub(super) fn template(key: &str, lang: Lang) -> &str {
    TEMPLATES
        .iter()
        .find(|(k, _)| *k == key)
        .map_or(key, |(_, t)| t[lang.index()])
}

/// A profession's name in `lang`.
pub fn profession_name(key: &str, lang: Lang) -> Option<&'static str> {
    PROFESSIONS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, t)| t[lang.index()])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Placeholders of a template, sorted.
    fn placeholders(t: &str) -> Vec<&str> {
        let mut out: Vec<&str> = t
            .split('{')
            .skip(1)
            .filter_map(|s| s.split_once('}').map(|(name, _)| name))
            .collect();
        out.sort_unstable();
        out
    }

    /// Every sentence exists in the 10 languages, with the same arguments as in English.
    #[test]
    fn every_language_has_every_sentence() {
        for (key, t) in TEMPLATES {
            for (lang, s) in t.iter().enumerate() {
                assert!(!s.is_empty(), "{key} in language {lang}");
                assert_eq!(placeholders(s), placeholders(t[0]), "{key} in language {lang}: {s}");
            }
        }
        assert_eq!(Lang::of("esMX"), Lang::Es);
        assert_eq!(Lang::of("xxXX"), Lang::En);
    }
}
