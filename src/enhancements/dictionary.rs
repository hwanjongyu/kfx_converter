//! Kindle Word Wise Dictionary & Vocabulary Provider in Rust
//!
//! Provides a built-in, curated CEFR B2-C2 academic vocabulary database for zero-dependency
//! standalone operation matching Amazon Kindle's Word Wise system.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub const ENGLISH_STOPWORDS: &[&str] = &[
    "a", "about", "above", "after", "again", "against", "all", "am", "an", "and",
    "any", "are", "aren't", "as", "at", "be", "because", "been", "before", "being",
    "below", "between", "both", "but", "by", "can", "can't", "cannot", "could",
    "couldn't", "did", "didn't", "do", "does", "doesn't", "doing", "don't", "down",
    "during", "each", "few", "for", "from", "further", "had", "hadn't", "has",
    "hasn't", "have", "haven't", "having", "he", "he'd", "he'll", "he's", "her",
    "here", "here's", "hers", "herself", "him", "himself", "his", "how", "how's",
    "i", "i'd", "i'll", "i'm", "i've", "if", "in", "into", "is", "isn't", "it",
    "it's", "its", "itself", "let's", "me", "more", "most", "mustn't", "my",
    "myself", "no", "nor", "not", "of", "off", "on", "once", "only", "or", "other",
    "ought", "our", "ours", "ourselves", "out", "over", "own", "same", "shan't",
    "she", "she'd", "she'll", "she's", "should", "shouldn't", "so", "some", "such",
    "than", "that", "that's", "the", "their", "theirs", "them", "themselves", "then",
    "there", "there's", "these", "they", "they'd", "they'll", "they're", "they've",
    "this", "those", "through", "to", "too", "under", "until", "up", "very", "was",
    "wasn't", "we", "we'd", "we'll", "we're", "we've", "were", "weren't", "what",
    "what's", "when", "when's", "where", "where's", "which", "while", "who", "who's",
    "whom", "why", "why's", "with", "won't", "would", "wouldn't", "you", "you'd",
    "you'll", "you're", "you've", "your", "yours", "yourself", "yourselves",
];

pub const BUILTIN_VOCABULARY: &[(&str, u8, u32)] = &[
    // Difficulty 3 (Intermediate-Advanced CEFR B2)
    ("abstruse", 3, 8435), ("acrimony", 3, 42592), ("amalgamate", 3, 16556),
    ("ameliorate", 3, 35386), ("anachronism", 3, 17750), ("anomalous", 3, 8502),
    ("arbitrate", 3, 15781), ("assiduous", 3, 34856), ("bathos", 3, 34892), ("belie", 3, 11857),
    ("bellwether", 3, 14092), ("bombastic", 3, 105389), ("calumny", 3, 15620),
    ("castigate", 3, 15728), ("caterwaul", 3, 7989), ("chicanery", 3, 15772),
    ("chimerical", 3, 100161), ("coalesce", 3, 29181), ("cogent", 3, 14904),
    ("commensurate", 3, 42620), ("consecrate", 3, 19445), ("credulous", 3, 7696),
    ("debilitate", 3, 35231), ("decimate", 3, 1081), ("defalcation", 3, 105478),
    ("deleterious", 3, 737), ("delineate", 3, 13735), ("demagogue", 3, 40611), ("demur", 3, 32524),
    ("denigrate", 3, 31898), ("deprecate", 3, 19474), ("deride", 3, 20151),
    ("desiccate", 3, 108536), ("didactic", 3, 4065), ("dilatory", 3, 18677),
    ("dilettante", 3, 39444), ("disabuse", 3, 16327), ("discernment", 3, 24150),
    ("disingenuous", 3, 23874), ("disparage", 3, 18954), ("disseminate", 3, 29603),
    ("dissonance", 3, 102184), ("divisive", 3, 8343), ("dogmatic", 3, 23031),
    ("ebullient", 3, 21852), ("effluvium", 3, 79964), ("egalitarian", 3, 26870),
    ("egregious", 3, 36506), ("elucidate", 3, 36391), ("enervate", 3, 5573), ("engender", 3, 4276),
    ("equivocal", 3, 42463), ("ersatz", 3, 19557), ("erudite", 3, 23071), ("evanescent", 3, 36739),
    ("exacerbate", 3, 6368), ("exasperate", 3, 14883), ("excoriate", 3, 18199),
    ("exonerate", 3, 12254), ("extol", 3, 29354), ("fallacious", 3, 22001), ("fatuous", 3, 13840),
    ("fecund", 3, 16194), ("filibuster", 3, 20069), ("flout", 3, 29581), ("foment", 3, 30194),
    ("fulsome", 3, 13830), ("gainsay", 3, 41827), ("galvanize", 3, 37142), ("germane", 3, 36025),
    ("grandiloquent", 3, 15173), ("hackneyed", 3, 10508), ("halcyon", 3, 27242),
    ("hebdomadal", 3, 105139), ("hedonist", 3, 42864), ("hegemony", 3, 10805),
    ("hermetic", 3, 3793), ("heterodox", 3, 25115), ("homogenous", 3, 13584),
    ("hyperbole", 3, 18817), ("iconoclast", 3, 668), ("idiosyncratic", 3, 94350),
    ("impair", 3, 6851), ("impecunious", 3, 37531), ("impiety", 3, 24184), ("inchoate", 3, 10617),
    ("incontrovertible", 3, 30395), ("ingenuous", 3, 14362), ("inimical", 3, 2200),
    ("innovate", 3, 37825), ("insouciance", 3, 8071), ("instigate", 3, 9675),
    ("inundate", 3, 22043), ("invidious", 3, 16850), ("irresolute", 3, 18362),
    ("juxtapose", 3, 36183), ("lachrymose", 3, 14195), ("laudable", 3, 14417),
    ("lugubrious", 3, 858), ("malediction", 3, 78859), ("maverick", 3, 36779),
    ("mendacious", 3, 35829), ("misanthrope", 3, 29802), ("munificent", 3, 36539),
    ("nadir", 3, 63630), ("nascent", 3, 64537), ("neophyte", 3, 49623), ("nullify", 3, 43304),
    ("obdurate", 3, 43251), ("obfuscate", 3, 56385), ("obstreperous", 3, 49459),
    ("oleaginous", 3, 104218), ("palliate", 3, 64223), ("panacea", 3, 65938),
    ("parsimonious", 3, 73758), ("parsimony", 3, 44441), ("paucity", 3, 76672),
    ("pellucid", 3, 63112), ("perfidious", 3, 70147), ("periphrastic", 3, 92192),
    ("perspicacious", 3, 44160), ("pervade", 3, 66335), ("phlegmatic", 3, 61131),
    ("piquant", 3, 53530), ("platitude", 3, 57036), ("preponderance", 3, 57763),
    ("prescient", 3, 46334), ("probity", 3, 74958), ("profligate", 3, 68333),
    ("propitious", 3, 53801), ("puerile", 3, 47885), ("pugnacious", 3, 54279),
    ("punctilious", 3, 56105), ("pusillanimous", 3, 76529), ("quixotic", 3, 55707),
    ("recant", 3, 77260), ("recondite", 3, 71322), ("redoubtable", 3, 74521),
    ("refractory", 3, 65992), ("refulgent", 3, 93846), ("relegate", 3, 56257),
    ("remonstrate", 3, 49647), ("reprobate", 3, 72853), ("repudiate", 3, 66867),
    ("sagacious", 3, 68514), ("salubrious", 3, 73480), ("sangfroid", 3, 49391),
    ("satiate", 3, 46398), ("sedulous", 3, 92194), ("serendipity", 3, 51902),
    ("sesquipedalian", 3, 99005), ("somnolent", 3, 44960), ("sophistry", 3, 73679),
    ("soporific", 3, 70259), ("specious", 3, 45824), ("stymie", 3, 54338), ("subjugate", 3, 51513),
    ("supplant", 3, 54586), ("surfeit", 3, 72817), ("sycophant", 3, 57820),
    ("tangential", 3, 77073), ("timorous", 3, 78170), ("torpid", 3, 45051),
    ("tractable", 3, 59837), ("trenchant", 3, 58752), ("truculent", 3, 73107),
    ("ululate", 3, 59079), ("umbrage", 3, 64401), ("unctuous", 3, 58508), ("underlie", 3, 74260),
    ("upbraid", 3, 59072), ("vacillate", 3, 52206), ("variegated", 3, 45701),
    ("venerate", 3, 76403), ("verbose", 3, 45759), ("vilify", 3, 50582), ("vindicate", 3, 64114),
    ("vitiate", 3, 53627), ("vociferous", 3, 71268), ("voluble", 3, 57348), ("winsome", 3, 43950),
    ("xenophobia", 3, 76471), ("zealot", 3, 60529), ("zeitgeist", 3, 59724), ("zephyr", 3, 51530),

    // Difficulty 4 (Advanced / Academic CEFR C1)
    ("aberration", 4, 10914), ("abhor", 4, 40675), ("acquiesce", 4, 19129), ("admonish", 4, 16197),
    ("alacrity", 4, 3846), ("ambiguous", 4, 24295), ("ambivalent", 4, 17619),
    ("antipathy", 4, 39281), ("antithesis", 4, 30589), ("apathy", 4, 38020),
    ("arbitrary", 4, 11315), ("arcane", 4, 20376), ("archaic", 4, 35794), ("arduous", 4, 13141),
    ("ascetic", 4, 19537), ("astonish", 4, 41837), ("audacious", 4, 16205), ("avarice", 4, 20190),
    ("brevity", 4, 34112), ("cajole", 4, 20417), ("capricious", 4, 36842), ("catalyst", 4, 31830),
    ("caustic", 4, 39615), ("censure", 4, 26343), ("circumspect", 4, 24001),
    ("clandestine", 4, 42181), ("coincide", 4, 16172), ("commence", 4, 6996), ("compel", 4, 1286),
    ("conciliatory", 4, 23664), ("concur", 4, 26594), ("condone", 4, 21201), ("conform", 4, 12674),
    ("connoisseur", 4, 9018), ("consequent", 4, 19262), ("contentious", 4, 4534),
    ("conundrum", 4, 38883), ("copious", 4, 19549), ("corroborate", 4, 8030),
    ("culpable", 4, 38005), ("cumbersome", 4, 35131), ("dearth", 4, 39461), ("decorum", 4, 15057),
    ("deferential", 4, 105264), ("deficient", 4, 35031), ("depict", 4, 5010),
    ("desultory", 4, 22437), ("deviate", 4, 6812), ("devise", 4, 151), ("diatribe", 4, 11204),
    ("diffident", 4, 17523), ("disclose", 4, 26233), ("disconsolate", 4, 16687),
    ("discordant", 4, 18327), ("discrepancy", 4, 31709), ("dispassionate", 4, 1271),
    ("distort", 4, 39473), ("divergent", 4, 105127), ("duplicity", 4, 7595),
    ("eclectic", 4, 28943), ("efficacy", 4, 37057), ("effrontery", 4, 24292), ("elicit", 4, 39242),
    ("embellish", 4, 19030), ("eminent", 4, 27254), ("emulate", 4, 31446), ("enigma", 4, 11524),
    ("ephemeral", 4, 3367), ("epiphany", 4, 13505), ("epitome", 4, 22340),
    ("equanimity", 4, 39967), ("eradicate", 4, 35729), ("esoteric", 4, 22280),
    ("esteemed", 4, 82075), ("eulogy", 4, 5636), ("euphemism", 4, 2964), ("exemplary", 4, 42821),
    ("exhaustive", 4, 28933), ("expedient", 4, 29320), ("extraneous", 4, 7373),
    ("facetious", 4, 23762), ("fastidious", 4, 26919), ("feasible", 4, 4278),
    ("flagrant", 4, 27627), ("fledgling", 4, 349), ("flippant", 4, 38386), ("forestall", 4, 12985),
    ("fortuitous", 4, 11812), ("fractious", 4, 8691), ("frugal", 4, 20383),
    ("gargantuan", 4, 28487), ("garrulous", 4, 13003), ("glib", 4, 25179), ("grandiose", 4, 22088),
    ("gratuitous", 4, 32766), ("gregarious", 4, 34693), ("grievous", 4, 30076), ("guile", 4, 6178),
    ("hapless", 4, 20878), ("harangue", 4, 27115), ("hector", 4, 42888), ("heretic", 4, 2413),
    ("hiatus", 4, 37727), ("hubris", 4, 36926), ("ignominious", 4, 21965), ("immutable", 4, 36199),
    ("impartial", 4, 30009), ("imperious", 4, 7754), ("impervious", 4, 5709),
    ("impetuous", 4, 30255), ("implacable", 4, 39830), ("implicit", 4, 9022),
    ("impudent", 4, 24876), ("inadvertent", 4, 17579), ("inane", 4, 6741), ("incipient", 4, 3502),
    ("incongruous", 4, 22752), ("indigenous", 4, 5574), ("indolent", 4, 42267),
    ("indomitable", 4, 19813), ("ineffable", 4, 9639), ("inept", 4, 31343), ("inert", 4, 15858),
    ("infer", 4, 24708), ("inhibit", 4, 2221), ("inscrutable", 4, 16699), ("insipid", 4, 43177),
    ("insolent", 4, 16107), ("insular", 4, 16737), ("intractable", 4, 4283),
    ("intrepid", 4, 34183), ("inured", 4, 102358), ("invective", 4, 9559),
    ("inveterate", 4, 29913), ("invoke", 4, 34839), ("irascible", 4, 5608),
    ("itinerant", 4, 15922), ("judicious", 4, 23932), ("laconic", 4, 35248), ("lament", 4, 23604),
    ("largesse", 4, 33995), ("latent", 4, 13423), ("lethargic", 4, 541), ("levity", 4, 32584),
    ("limpid", 4, 6982), ("magnanimous", 4, 28211), ("malleable", 4, 43137),
    ("mercurial", 4, 31413), ("mitigate", 4, 16026), ("mollify", 4, 5381), ("morose", 4, 43063),
    ("nebulous", 4, 69276), ("nefarious", 4, 53794), ("negligent", 4, 72001),
    ("nocturnal", 4, 57714), ("noxious", 4, 56506), ("obsequious", 4, 73019),
    ("obsolete", 4, 77545), ("obstinate", 4, 61214), ("obtuse", 4, 48468), ("onerous", 4, 59661),
    ("opulent", 4, 69290), ("ostentatious", 4, 75340), ("paragon", 4, 44503), ("pariah", 4, 68546),
    ("partisan", 4, 69248), ("pedantic", 4, 104833), ("perfunctory", 4, 78270),
    ("pernicious", 4, 77133), ("peruse", 4, 62177), ("pervasive", 4, 43456), ("placate", 4, 71732),
    ("plethora", 4, 58728), ("poignant", 4, 59985), ("pragmatic", 4, 48523),
    ("preclude", 4, 52069), ("precocious", 4, 77456), ("predilection", 4, 69679),
    ("predominant", 4, 49212), ("presumptuous", 4, 56003), ("prodigal", 4, 53553),
    ("prodigious", 4, 74954), ("profuse", 4, 62884), ("prolific", 4, 51942),
    ("propensity", 4, 55829), ("prosaic", 4, 51586), ("prosper", 4, 74179),
    ("provincial", 4, 55104), ("quandary", 4, 63103), ("querulous", 4, 68210),
    ("quiescent", 4, 74980), ("rancor", 4, 56812), ("rapacious", 4, 58149),
    ("recalcitrant", 4, 46132), ("refine", 4, 60619), ("reiterate", 4, 47445),
    ("renounce", 4, 78262), ("reprehensible", 4, 75851), ("rescind", 4, 74338),
    ("resilient", 4, 57399), ("reticent", 4, 60499), ("revere", 4, 59578), ("ribald", 4, 54218),
    ("rife", 4, 64918), ("rudimentary", 4, 59106), ("salient", 4, 65234),
    ("sanctimonious", 4, 52462), ("sanguine", 4, 55160), ("scintillating", 4, 68937),
    ("scrupulous", 4, 67819), ("scrutinize", 4, 55161), ("servile", 4, 58840),
    ("sinuous", 4, 57917), ("solicitous", 4, 46448), ("sonorous", 4, 66600),
    ("spurious", 4, 53788), ("squalid", 4, 61215), ("staid", 4, 60310), ("strident", 4, 56393),
    ("sublime", 4, 65901), ("subservient", 4, 62644), ("substantiate", 4, 77572),
    ("subversive", 4, 64806), ("succinct", 4, 51989), ("supercilious", 4, 71294),
    ("superfluous", 4, 53511), ("surreptitious", 4, 73110), ("tacit", 4, 50744),
    ("taciturn", 4, 44246), ("tawdry", 4, 70208), ("temerity", 4, 72052), ("tenacious", 4, 48674),
    ("transient", 4, 77922), ("turgid", 4, 62331), ("ubiquitous", 4, 52437),
    ("unequivocal", 4, 70727), ("unfettered", 4, 46626), ("untoward", 4, 49055),
    ("urbane", 4, 46904), ("usurp", 4, 45337), ("vacuous", 4, 69836), ("vapid", 4, 64511),
    ("vehement", 4, 49089), ("veracity", 4, 60108), ("verdant", 4, 69740), ("vestige", 4, 64146),
    ("vexation", 4, 57106), ("vicarious", 4, 70338), ("virulent", 4, 51186), ("viscous", 4, 46584),
    ("voracious", 4, 73677), ("wane", 4, 69830), ("wizened", 4, 53455), ("yoke", 4, 45411),
    ("zenith", 4, 76245),

    // Difficulty 5 (Proficiency / Rare / GRE CEFR C2)
    ("abundant", 5, 6756), ("adequate", 5, 6120), ("aesthetic", 5, 36175), ("alleviate", 5, 8055),
    ("anticipate", 5, 8729), ("apparent", 5, 14117), ("appease", 5, 40934),
    ("articulate", 5, 25737), ("assert", 5, 14264), ("attain", 5, 10391), ("austere", 5, 4121),
    ("authentic", 5, 12623), ("belligerent", 5, 23239), ("benevolent", 5, 30789),
    ("benign", 5, 14432), ("cacophony", 5, 9795), ("cease", 5, 2688), ("coherent", 5, 41136),
    ("complacent", 5, 34186), ("comprehensive", 5, 41506), ("conceal", 5, 31359),
    ("consent", 5, 12349), ("conspicuous", 5, 36650), ("contemplate", 5, 4756),
    ("contradict", 5, 22113), ("crucial", 5, 751), ("cursory", 5, 19788), ("daunting", 5, 37877),
    ("deceive", 5, 3431), ("deliberate", 5, 21843), ("derive", 5, 12563), ("deter", 5, 6440),
    ("diminish", 5, 18524), ("discern", 5, 29273), ("discreet", 5, 28641), ("disdain", 5, 16744),
    ("diverse", 5, 42936), ("docile", 5, 13436), ("dormant", 5, 6990), ("dubious", 5, 18148),
    ("elaborate", 5, 18417), ("eloquent", 5, 39084), ("elusive", 5, 31846), ("empathy", 5, 20840),
    ("empirical", 5, 30481), ("endure", 5, 1197), ("enhance", 5, 22868), ("evident", 5, 43103),
    ("explicit", 5, 12638), ("extravagant", 5, 21949), ("facilitate", 5, 36452),
    ("fervent", 5, 14071), ("flourish", 5, 9688), ("formidable", 5, 37678), ("foster", 5, 25818),
    ("furtive", 5, 1620), ("futile", 5, 2315), ("genuine", 5, 21375), ("haughty", 5, 35004),
    ("hesitate", 5, 5308), ("hierarchy", 5, 27762), ("hostile", 5, 14994), ("illicit", 5, 42541),
    ("immense", 5, 7997), ("imminent", 5, 38515), ("impassive", 5, 21320), ("impose", 5, 945),
    ("indifferent", 5, 13635), ("induce", 5, 31464), ("inevitable", 5, 6734),
    ("infamous", 5, 6157), ("inherent", 5, 6449), ("innocuous", 5, 34733), ("insight", 5, 43044),
    ("integrate", 5, 32170), ("intense", 5, 22987), ("intervene", 5, 30168),
    ("intricate", 5, 19991), ("languid", 5, 35122), ("lucid", 5, 18071), ("luminous", 5, 7494),
    ("malevolent", 5, 25080), ("manifest", 5, 17957), ("melancholy", 5, 21478),
    ("meticulous", 5, 3115), ("mundane", 5, 3770), ("myriad", 5, 4735), ("nonchalant", 5, 45910),
    ("nostalgia", 5, 77740), ("notable", 5, 49850), ("oblivious", 5, 54014), ("obscure", 5, 48688),
    ("obtain", 5, 47023), ("ominous", 5, 67981), ("opaque", 5, 54363), ("paradigm", 5, 66242),
    ("penchant", 5, 51197), ("perceive", 5, 51768), ("persist", 5, 61561), ("petulant", 5, 60286),
    ("plausible", 5, 70711), ("ponder", 5, 58435), ("precarious", 5, 57143), ("precede", 5, 59713),
    ("precise", 5, 60389), ("prevail", 5, 73663), ("profound", 5, 54502), ("prominent", 5, 47249),
    ("propriety", 5, 62890), ("provoke", 5, 69473), ("prudent", 5, 53379), ("pungent", 5, 44935),
    ("pursue", 5, 68585), ("quell", 5, 57221), ("rational", 5, 54323), ("raucous", 5, 64536),
    ("rebuke", 5, 67582), ("reluctant", 5, 55426), ("resemble", 5, 48774), ("restrain", 5, 62551),
    ("rhetoric", 5, 72759), ("rigid", 5, 47949), ("robust", 5, 63202), ("scarcely", 5, 55533),
    ("sincere", 5, 52396), ("skepticism", 5, 51891), ("solitary", 5, 50799), ("stoic", 5, 72645),
    ("subsequent", 5, 65215), ("subtle", 5, 48265), ("suppress", 5, 45490), ("sustain", 5, 49113),
    ("tedious", 5, 50093), ("tentative", 5, 59946), ("tenuous", 5, 49946), ("tolerate", 5, 63145),
    ("trepidation", 5, 64538), ("trigger", 5, 60422), ("undermine", 5, 63315),
    ("unprecedented", 5, 63111), ("utilize", 5, 53528), ("valid", 5, 55287), ("vanish", 5, 50145),
    ("vital", 5, 46110), ("vivid", 5, 45015), ("volatile", 5, 62706), ("wan", 5, 61771),
    ("wanton", 5, 65555), ("wistful", 5, 65423),
];

static STOPWORDS_SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
static VOCAB_MAP: OnceLock<HashMap<&'static str, (u8, u32)>> = OnceLock::new();

pub struct LemmaDictionary;

impl LemmaDictionary {
    pub fn new() -> Self {
        STOPWORDS_SET.get_or_init(|| ENGLISH_STOPWORDS.iter().copied().collect());
        VOCAB_MAP.get_or_init(|| {
            BUILTIN_VOCABULARY
                .iter()
                .map(|&(w, d, s)| (w, (d, s)))
                .collect()
        });
        Self
    }

    /// Looks up a word candidate in the dictionary.
    /// Normalizes punctuation and attempts standard English suffix stemming.
    pub fn lookup(&self, word: &str, min_word_len: usize) -> Option<(u8, u32)> {
        let stopwords = STOPWORDS_SET.get_or_init(|| ENGLISH_STOPWORDS.iter().copied().collect());
        let vocab = VOCAB_MAP.get_or_init(|| {
            BUILTIN_VOCABULARY
                .iter()
                .map(|&(w, d, s)| (w, (d, s)))
                .collect()
        });

        let mut clean = word.trim().trim_matches(&['\'', '’', '"', '“', '”', '`', ',', '.', ';', ':', '!', '?'][..]).to_lowercase();
        if let Some(stripped) = clean.strip_suffix("'s").or_else(|| clean.strip_suffix("’s")) {
            clean = stripped.to_string();
        }

        if clean.len() < min_word_len || stopwords.contains(clean.as_str()) {
            return None;
        }

        // Direct lookup
        if let Some(&res) = vocab.get(clean.as_str()) {
            return Some(res);
        }

        // Suffix stemming fallback
        for suffix in &["ed", "ing", "ly", "es", "s"] {
            if clean.ends_with(suffix) {
                let stem = &clean[..clean.len() - suffix.len()];
                if stem.len() >= min_word_len {
                    if let Some(&res) = vocab.get(stem) {
                        return Some(res);
                    }
                }
                // Try with trailing 'e' (e.g. "ameliorating" -> "ameliorat" -> "ameliorate")
                let with_e = format!("{}e", stem);
                if with_e.len() >= min_word_len {
                    if let Some(&res) = vocab.get(with_e.as_str()) {
                        return Some(res);
                    }
                }
                // Doubled consonant (e.g. "compelled" -> "compel")
                if stem.len() > 2 {
                    let bytes = stem.as_bytes();
                    if bytes[bytes.len() - 1] == bytes[bytes.len() - 2] {
                        let single = &stem[..stem.len() - 1];
                        if single.len() >= min_word_len {
                            if let Some(&res) = vocab.get(single) {
                                return Some(res);
                            }
                        }
                    }
                }
            }
        }

        None
    }
}

impl Default for LemmaDictionary {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lemma_dictionary_lookups() {
        let dict = LemmaDictionary::new();

        // 1. Direct hit
        let res = dict.lookup("ameliorate", 3);
        assert_eq!(res, Some((3, 35386)));

        // 2. Inflected form (-ing with added 'e')
        let res2 = dict.lookup("ameliorating", 3);
        assert_eq!(res2, Some((3, 35386)));

        // 3. Stopword filtered
        assert_eq!(dict.lookup("the", 3), None);
        assert_eq!(dict.lookup("because", 3), None);

        // 4. Short word filtered
        assert_eq!(dict.lookup("it", 3), None);

        // 5. Doubled consonant
        let res3 = dict.lookup("compelled", 3);
        assert_eq!(res3, Some((4, 1286)));

        // 6. Smart apostrophe (UTF-8 3-byte curly quote) - must not panic
        let res4 = dict.lookup("ameliorate’s", 3);
        assert_eq!(res4, Some((3, 35386)));
        let res5 = dict.lookup("ameliorate's", 3);
        assert_eq!(res5, Some((3, 35386)));
    }
}
