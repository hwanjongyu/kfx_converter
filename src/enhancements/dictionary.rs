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
    // Difficulty 1 (GRE / CEFR C2 Rare & Arcane - shown at lowest slider 'Fewer Hints')
    ("abstruse", 1, 8435), ("acrimony", 1, 42592), ("amalgamate", 1, 16556),
    ("ameliorate", 1, 35386), ("anachronism", 1, 17750), ("anomalous", 1, 8502),
    ("arbitrate", 1, 15781), ("assiduous", 1, 34856), ("bathos", 1, 34892), ("belie", 1, 11857),
    ("bellwether", 1, 14092), ("bombastic", 1, 105389), ("calumny", 1, 15620),
    ("camphor", 1, 38030), ("castigate", 1, 15728), ("caterwaul", 1, 7989),
    ("chicanery", 1, 15772), ("chimerical", 1, 100161), ("clamber", 1, 36016),
    ("coachman", 1, 26549), ("coalesce", 1, 29181), ("cogent", 1, 14904),
    ("colossal", 1, 13955), ("commensurate", 1, 42620), ("consecrate", 1, 19445),
    ("credulous", 1, 7696), ("debilitate", 1, 35231), ("decimate", 1, 1081),
    ("defalcation", 1, 105478), ("deleterious", 1, 737), ("delineate", 1, 13735),
    ("demagogue", 1, 40611), ("demur", 1, 32524), ("denigrate", 1, 31898),
    ("deprecate", 1, 19474), ("deride", 1, 20151), ("desiccate", 1, 108536),
    ("didactic", 1, 4065), ("dilatory", 1, 18677), ("dilettante", 1, 39444),
    ("disabuse", 1, 16327), ("discernment", 1, 24150), ("disingenuous", 1, 23874),
    ("disparage", 1, 18954), ("disseminate", 1, 29603), ("dissonance", 1, 102184),
    ("divisive", 1, 8343), ("dogmatic", 1, 23031), ("ebullient", 1, 21852),
    ("effluvium", 1, 79964), ("egalitarian", 1, 26870), ("egregious", 1, 36506),
    ("elucidate", 1, 36391), ("enervate", 1, 5573), ("engender", 1, 4276),
    ("equivocal", 1, 42463), ("ersatz", 1, 19557), ("erudite", 1, 23071),
    ("evanescent", 1, 36739), ("exacerbate", 1, 6368), ("exasperate", 1, 14883),
    ("excoriate", 1, 18199), ("exonerate", 1, 12254), ("extol", 1, 29354),
    ("fallacious", 1, 22001), ("fatuous", 1, 13840), ("fecund", 1, 16194),
    ("filibuster", 1, 20069), ("flout", 1, 29581), ("foment", 1, 30194),
    ("fulsome", 1, 13830), ("gainsay", 1, 41827), ("galvanize", 1, 37142),
    ("germane", 1, 36025), ("grandiloquent", 1, 15173), ("hackneyed", 1, 10508),
    ("halcyon", 1, 27242), ("hazy", 1, 40077), ("headlong", 1, 19647),
    ("hebdomadal", 1, 105139), ("hedonist", 1, 42864), ("hegemony", 1, 10805),
    ("hermetic", 1, 3793), ("heterodox", 1, 25115), ("hillock", 1, 34195),
    ("homogenous", 1, 13584), ("hyperbole", 1, 18817), ("iconoclast", 1, 668),
    ("idiosyncratic", 1, 94350), ("impair", 1, 6851), ("impecunious", 1, 37531),
    ("impiety", 1, 24184), ("inchoate", 1, 10617), ("incontrovertible", 1, 30395),
    ("incredulous", 1, 32470), ("indistinct", 1, 2931), ("ingenuous", 1, 14362),
    ("inimical", 1, 2200), ("innovate", 1, 37825), ("insouciance", 1, 8071),
    ("instigate", 1, 9675), ("interminable", 1, 6624), ("inundate", 1, 22043),
    ("invidious", 1, 16850), ("irresolute", 1, 18362), ("jaundice", 1, 1306),
    ("jaundiced", 1, 19346), ("juxtapose", 1, 36183),
    ("lachrymose", 1, 14195), ("laudable", 1, 14417), ("lectern", 1, 8634),
    ("lugubrious", 1, 858), ("malediction", 1, 78859), ("maverick", 1, 36779),
    ("mendacious", 1, 35829), ("misanthrope", 1, 29802), ("mischievous", 1, 4629),
    ("munificent", 1, 36539), ("nadir", 1, 63630), ("nascent", 1, 64537),
    ("neophyte", 1, 49623), ("nullify", 1, 43304), ("obdurate", 1, 43251),
    ("obfuscate", 1, 56385), ("obstreperous", 1, 49459), ("oleaginous", 1, 104218),
    ("outlandish", 1, 48083), ("palliate", 1, 64223), ("panacea", 1, 65938),
    ("parsimonious", 1, 73758), ("parsimony", 1, 44441), ("paucity", 1, 76672),
    ("pellucid", 1, 63112), ("perfidious", 1, 70147), ("periphrastic", 1, 92192),
    ("perspicacious", 1, 44160), ("pervade", 1, 66335), ("phlegmatic", 1, 61131),
    ("piquant", 1, 53530), ("platitude", 1, 57036), ("preponderance", 1, 57763),
    ("prescient", 1, 46334), ("probity", 1, 74958), ("profligate", 1, 68333),
    ("propitious", 1, 53801), ("puerile", 1, 47885), ("pugnacious", 1, 54279),
    ("punctilious", 1, 56105), ("pusillanimous", 1, 76529), ("quixotic", 1, 55707),
    ("recant", 1, 77260), ("recondite", 1, 71322), ("redoubtable", 1, 74521),
    ("refractory", 1, 65992), ("refulgent", 1, 93846), ("relegate", 1, 56257),
    ("remonstrate", 1, 49647), ("reprobate", 1, 72853), ("repudiate", 1, 66867),
    ("ruinous", 1, 69463), ("sagacious", 1, 68514), ("salubrious", 1, 73480),
    ("sangfroid", 1, 49391), ("satiate", 1, 46398), ("sedulous", 1, 92194),
    ("serendipity", 1, 51902), ("sesquipedalian", 1, 99005), ("somber", 1, 49100),
    ("somnolent", 1, 44960), ("sophistry", 1, 73679), ("soporific", 1, 70259),
    ("specious", 1, 45824), ("sphinx", 1, 58703), ("stymie", 1, 54338),
    ("subjugate", 1, 51513), ("supplant", 1, 54586), ("surfeit", 1, 72817),
    ("swig", 1, 69402), ("sycophant", 1, 57820), ("tangential", 1, 77073),
    ("timorous", 1, 78170), ("torpid", 1, 45051), ("tractable", 1, 59837),
    ("trenchant", 1, 58752), ("truculent", 1, 73107), ("ululate", 1, 59079),
    ("umbrage", 1, 64401), ("unctuous", 1, 58508), ("underlie", 1, 74260),
    ("undoubted", 1, 73317), ("unease", 1, 57849), ("upbraid", 1, 59072),
    ("vacillate", 1, 52206), ("variegated", 1, 45701), ("venerate", 1, 76403),
    ("verbose", 1, 45759), ("vestige", 1, 64146), ("vilify", 1, 50582),
    ("vindicate", 1, 64114), ("vitiate", 1, 53627), ("vociferous", 1, 71268),
    ("voluble", 1, 57348), ("winsome", 1, 43950), ("wry", 1, 51966),
    ("xenophobia", 1, 76471), ("zealot", 1, 60529), ("zeitgeist", 1, 59724),
    ("zephyr", 1, 51530),

    // Difficulty 2 (Advanced CEFR C1 Academic & Literary)
    ("aberration", 2, 10914), ("abhor", 2, 40675), ("acquiesce", 2, 19129),
    ("admonish", 2, 16197), ("alacrity", 2, 3846), ("amid", 2, 4814),
    ("antipathy", 2, 39281), ("antithesis", 2, 30589), ("apathy", 2, 38020),
    ("arcane", 2, 20376), ("archaic", 2, 35794), ("arduous", 2, 13141),
    ("ascetic", 2, 19537), ("astonish", 2, 41837), ("audacious", 2, 16205),
    ("automaton", 2, 35511), ("avarice", 2, 20190), ("brevity", 2, 34112),
    ("cajole", 2, 20417), ("capricious", 2, 36842), ("caustic", 2, 39615),
    ("censure", 2, 26343), ("circumspect", 2, 24001), ("clandestine", 2, 42181),
    ("conciliatory", 2, 23664), ("concur", 2, 26594), ("condone", 2, 21201),
    ("connoisseur", 2, 9018), ("contentious", 2, 4534), ("conundrum", 2, 38883),
    ("copious", 2, 19549), ("corroborate", 2, 8030), ("culpable", 2, 38005),
    ("cumbersome", 2, 35131), ("dearth", 2, 39461), ("decorum", 2, 15057),
    ("deferential", 2, 105264), ("desultory", 2, 22437), ("diatribe", 2, 11204),
    ("diffident", 2, 17523), ("disconsolate", 2, 16687), ("discordant", 2, 18327),
    ("discrepancy", 2, 31709), ("dispassionate", 2, 1271), ("divergent", 2, 105127),
    ("duplicity", 2, 7595), ("eclectic", 2, 28943), ("efficacy", 2, 37057),
    ("effrontery", 2, 24292), ("elicit", 2, 39242), ("embellish", 2, 19030),
    ("emulate", 2, 31446), ("enigma", 2, 11524), ("ephemeral", 2, 3367),
    ("epiphany", 2, 13505), ("epitome", 2, 22340), ("equanimity", 2, 39967),
    ("eradicate", 2, 35729), ("esoteric", 2, 22280), ("eulogy", 2, 5636),
    ("euphemism", 2, 2964), ("exemplary", 2, 42821), ("fastidious", 2, 26919),
    ("flagrant", 2, 27627), ("fledgling", 2, 349), ("flippant", 2, 38386),
    ("forestall", 2, 12985), ("fortuitous", 2, 11812), ("fractious", 2, 8691),
    ("frugal", 2, 20383), ("gargantuan", 2, 28487), ("garrulous", 2, 13003),
    ("glib", 2, 25179), ("grandiose", 2, 22088), ("gratuitous", 2, 32766),
    ("gregarious", 2, 34693), ("grievous", 2, 30076), ("guile", 2, 6178),
    ("hapless", 2, 20878), ("harangue", 2, 27115), ("hector", 2, 42888),
    ("heretic", 2, 2413), ("hiatus", 2, 37727), ("hubris", 2, 36926),
    ("ignominious", 2, 21965), ("immutable", 2, 36199), ("imperious", 2, 7754),
    ("impervious", 2, 5709), ("impetuous", 2, 30255), ("implacable", 2, 39830),
    ("impudent", 2, 24876), ("inane", 2, 6741), ("incipient", 2, 3502),
    ("incongruous", 2, 22752), ("indolent", 2, 42267), ("indomitable", 2, 19813),
    ("ineffable", 2, 9639), ("inscrutable", 2, 16699), ("insipid", 2, 43177),
    ("insolent", 2, 16107), ("insular", 2, 16737), ("intractable", 2, 4283),
    ("intrepid", 2, 34183), ("inured", 2, 102358), ("invective", 2, 9559),
    ("inveterate", 2, 29913), ("irascible", 2, 5608), ("itinerant", 2, 15922),
    ("judicious", 2, 23932), ("laconic", 2, 35248), ("largesse", 2, 33995),
    ("levity", 2, 32584), ("limpid", 2, 6982), ("magnanimous", 2, 28211),
    ("malleable", 2, 43137), ("mercurial", 2, 31413), ("mitigate", 2, 16026),
    ("mollify", 2, 5381), ("morose", 2, 43063), ("nebulous", 2, 69276),
    ("nefarious", 2, 53794), ("obsequious", 2, 73019), ("obstinate", 2, 61214),
    ("obtuse", 2, 48468), ("onerous", 2, 59661), ("opulent", 2, 69290),
    ("ostentatious", 2, 75340), ("paragon", 2, 44503), ("pariah", 2, 68546),
    ("pedantic", 2, 104833), ("perfunctory", 2, 78270), ("pernicious", 2, 77133),
    ("peruse", 2, 62177), ("placate", 2, 71732), ("plethora", 2, 58728),
    ("poignant", 2, 59985), ("precocious", 2, 77456), ("predilection", 2, 69679),
    ("presumptuous", 2, 56003), ("prodigal", 2, 53553), ("prodigious", 2, 74954),
    ("querulous", 2, 68210), ("quiescent", 2, 74980), ("rancor", 2, 56812),
    ("rapacious", 2, 58149), ("recalcitrant", 2, 46132), ("reprehensible", 2, 75851),
    ("rescind", 2, 74338), ("ribald", 2, 54218), ("rife", 2, 64918),
    ("rudimentary", 2, 59106), ("salient", 2, 65234), ("sanctimonious", 2, 52462),
    ("sanguine", 2, 55160), ("scintillating", 2, 68937), ("scrupulous", 2, 67819),
    ("sinuous", 2, 57917), ("solicitous", 2, 46448), ("sonorous", 2, 66600),
    ("spurious", 2, 53788), ("squalid", 2, 61215), ("staid", 2, 60310),
    ("strident", 2, 56393), ("sublime", 2, 65901), ("subservient", 2, 62644),
    ("subversive", 2, 64806), ("succinct", 2, 51989), ("supercilious", 2, 71294),
    ("superfluous", 2, 53511), ("surreptitious", 2, 73110), ("tacit", 2, 50744),
    ("taciturn", 2, 44246), ("tawdry", 2, 70208), ("temerity", 2, 72052),
    ("tenacious", 2, 48674), ("transient", 2, 77922), ("turgid", 2, 62331),
    ("ubiquitous", 2, 52437), ("unequivocal", 2, 70727), ("unfettered", 2, 46626),
    ("untoward", 2, 49055), ("urbane", 2, 46904), ("usurp", 2, 45337),
    ("vacuous", 2, 69836), ("vapid", 2, 64511), ("vehement", 2, 49089),
    ("veracity", 2, 60108), ("verdant", 2, 69740), ("vexation", 2, 57106),
    ("vicarious", 2, 70338), ("virulent", 2, 51186), ("viscous", 2, 46584),
    ("voracious", 2, 73677), ("wane", 2, 69830), ("wizened", 2, 53455),
    ("yoke", 2, 45411), ("zenith", 2, 76245),

    // Difficulty 3 (Upper-Intermediate CEFR B2/C1 - Balanced baseline)
    ("ambiguous", 3, 24295), ("ambivalent", 3, 17619), ("arbitrary", 3, 11315),
    ("benevolent", 3, 30789), ("catalyst", 3, 31830), ("coincide", 3, 16172),
    ("commence", 3, 6996), ("compel", 3, 1286), ("conform", 3, 12674),
    ("consequent", 3, 19262), ("contemplate", 3, 4756), ("deficient", 3, 35031),
    ("depict", 3, 5010), ("deviate", 3, 6812), ("devise", 3, 151),
    ("disclose", 3, 26233), ("distort", 3, 39473), ("eminent", 3, 27254),
    ("esteemed", 3, 82075), ("exhaustive", 3, 28933), ("expedient", 3, 29320),
    ("extraneous", 3, 7373), ("facetious", 3, 23762), ("feasible", 3, 4278),
    ("impartial", 3, 30009), ("implicit", 3, 9022), ("inadvertent", 3, 17579),
    ("incapable", 3, 9), ("indigenous", 3, 5574), ("inept", 3, 31343),
    ("inert", 3, 15858), ("infer", 3, 24708), ("inhibit", 3, 2221),
    ("invoke", 3, 34839), ("lament", 3, 23604), ("latent", 3, 13423),
    ("lethargic", 3, 541), ("lever", 3, 25), ("mansion", 3, 9),
    ("negligent", 3, 72001), ("nocturnal", 3, 57714), ("noxious", 3, 56506),
    ("obsolete", 3, 77545), ("partisan", 3, 69248), ("pervasive", 3, 43456),
    ("pragmatic", 3, 48523), ("preclude", 3, 52069), ("predominant", 3, 49212),
    ("profuse", 3, 62884), ("prolific", 3, 51942), ("propensity", 3, 55829),
    ("prosaic", 3, 51586), ("prosper", 3, 74179), ("provincial", 3, 55104),
    ("quandary", 3, 63103), ("refine", 3, 60619), ("reiterate", 3, 47445),
    ("renounce", 3, 78262), ("resilient", 3, 57399), ("reticent", 3, 60499),
    ("revere", 3, 59578), ("scarcely", 3, 55533), ("scrutinize", 3, 55161),
    ("servile", 3, 58840), ("shiver", 3, 10), ("splendid", 3, 29354),
    ("substantiate", 3, 77572),

    // Difficulty 4 (Intermediate CEFR B2 Vocabulary)
    ("abundant", 4, 6756), ("adequate", 4, 6120), ("aesthetic", 4, 36175),
    ("alleviate", 4, 8055), ("anticipate", 4, 8729), ("apparent", 4, 14117),
    ("appease", 4, 40934), ("articulate", 4, 25737), ("assert", 4, 14264),
    ("attain", 4, 10391), ("austere", 4, 4121), ("authentic", 4, 12623),
    ("belligerent", 4, 23239), ("benign", 4, 14432), ("cacophony", 4, 9795),
    ("coherent", 4, 41136), ("complacent", 4, 34186), ("comprehensive", 4, 41506),
    ("conspicuous", 4, 36650), ("contradict", 4, 22113), ("crucial", 4, 751),
    ("cursory", 4, 19788), ("deliberate", 4, 21843), ("diminish", 4, 18524),
    ("discern", 4, 29273), ("discreet", 4, 28641), ("disdain", 4, 16744),
    ("diverse", 4, 42936), ("elaborate", 4, 18417), ("eloquent", 4, 39084),
    ("elusive", 4, 31846), ("empathy", 4, 20840), ("empirical", 4, 30481),
    ("enhance", 4, 22868), ("evident", 4, 43103), ("exclusive", 4, 40687),
    ("explicit", 4, 12638), ("extravagant", 4, 21949), ("facilitate", 4, 36452),
    ("fervent", 4, 14071), ("flourish", 4, 9688), ("formidable", 4, 37678),
    ("foster", 4, 25818), ("furtive", 4, 1620), ("futile", 4, 2315),
    ("haughty", 4, 35004), ("hierarchy", 4, 27762), ("illicit", 4, 42541),
    ("imminent", 4, 38515), ("impassive", 4, 21320), ("impose", 4, 945),
    ("indifferent", 4, 13635), ("induce", 4, 31464), ("inevitable", 4, 6734),
    ("infamous", 4, 6157), ("inherent", 4, 6449), ("innocuous", 4, 34733),
    ("insight", 4, 43044), ("integrate", 4, 32170), ("intervene", 4, 30168),
    ("intricate", 4, 19991), ("languid", 4, 35122), ("lucid", 4, 18071),
    ("luminous", 4, 7494), ("malevolent", 4, 25080), ("manifest", 4, 17957),
    ("melancholy", 4, 21478), ("meticulous", 4, 3115), ("mundane", 4, 3770),
    ("myriad", 4, 4735), ("nonchalant", 4, 45910), ("nostalgia", 4, 77740),
    ("notable", 4, 49850), ("oblivious", 4, 54014), ("obscure", 4, 48688),
    ("obtain", 4, 47023), ("ominous", 4, 67981), ("opaque", 4, 54363),
    ("paradigm", 4, 66242), ("penchant", 4, 51197), ("perceive", 4, 51768),
    ("petulant", 4, 60286), ("plausible", 4, 70711), ("ponder", 4, 58435),
    ("precarious", 4, 57143), ("precede", 4, 59713), ("precise", 4, 60389),
    ("prevail", 4, 73663), ("profound", 4, 54502), ("prominent", 4, 47249),
    ("propriety", 4, 62890), ("pungent", 4, 44935), ("quell", 4, 57221),
    ("raucous", 4, 64536), ("rebuke", 4, 67582), ("reluctant", 4, 55426),
    ("rigid", 4, 47949), ("robust", 4, 63202), ("skepticism", 4, 51891),
    ("stoic", 4, 72645), ("subsequent", 4, 65215), ("subtle", 4, 48265),
    ("suppress", 4, 45490), ("sustain", 4, 49113), ("tedious", 4, 50093),
    ("tentative", 4, 59946), ("tenuous", 4, 49946), ("trepidation", 4, 64538),
    ("unprecedented", 4, 63111), ("wan", 4, 61771), ("wanton", 4, 65555),
    ("wistful", 4, 65423),

    // Difficulty 5 (Foundational Academic Vocabulary - shown on highest slider 'More Hints')
    ("cease", 5, 2688), ("click", 5, 42955), ("conceal", 5, 31359),
    ("consent", 5, 12349), ("daunting", 5, 37877), ("deceive", 5, 3431),
    ("derive", 5, 12563), ("deter", 5, 6440), ("docile", 5, 13436),
    ("dormant", 5, 6990), ("download", 5, 28861), ("dubious", 5, 18148),
    ("endure", 5, 1197), ("genuine", 5, 21375), ("hesitate", 5, 5308),
    ("hostile", 5, 14994), ("housewife", 5, 7958), ("immense", 5, 7997), ("intense", 5, 22987),
    ("persist", 5, 61561), ("provoke", 5, 69473), ("prudent", 5, 53379),
    ("pursue", 5, 68585), ("rational", 5, 54323), ("resemble", 5, 48774),
    ("restrain", 5, 62551), ("rhetoric", 5, 72759), ("sincere", 5, 52396),
    ("solitary", 5, 50799), ("tolerate", 5, 63145), ("trigger", 5, 60422),
    ("undermine", 5, 63315), ("utilize", 5, 53528), ("valid", 5, 55287),
    ("vanish", 5, 50145), ("vital", 5, 46110), ("vivid", 5, 45015),
    ("volatile", 5, 62706),

];

static WORDWISE_DICT_BYTES: &[u8] = include_bytes!("wordwise_dict.bin");

static STOPWORDS_SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
static VOCAB_MAP: OnceLock<HashMap<&'static str, (u8, u32)>> = OnceLock::new();

fn parse_builtin_and_ground_truth_vocab() -> HashMap<&'static str, (u8, u32)> {
    let mut map = HashMap::with_capacity(22_000);

    // 1. Load curated baseline overrides
    for &(w, d, s) in BUILTIN_VOCABULARY {
        map.insert(w, (d, s));
    }

    // 2. Parse Amazon Ground Truth binary dataset (20,000+ lemmas with native sense IDs)
    if WORDWISE_DICT_BYTES.len() >= 8 && &WORDWISE_DICT_BYTES[0..4] == b"WWK1" {
        let count = u32::from_le_bytes(WORDWISE_DICT_BYTES[4..8].try_into().unwrap()) as usize;
        let mut offset = 8;
        for _ in 0..count {
            if offset >= WORDWISE_DICT_BYTES.len() {
                break;
            }
            let len = WORDWISE_DICT_BYTES[offset] as usize;
            offset += 1;
            if offset + len + 5 > WORDWISE_DICT_BYTES.len() {
                break;
            }
            if let Ok(lemma) = std::str::from_utf8(&WORDWISE_DICT_BYTES[offset..offset + len]) {
                offset += len;
                let diff = WORDWISE_DICT_BYTES[offset];
                offset += 1;
                let sense_id = u32::from_le_bytes(
                    WORDWISE_DICT_BYTES[offset..offset + 4].try_into().unwrap(),
                );
                offset += 4;
                map.entry(lemma).or_insert((diff, sense_id));
            } else {
                offset += len + 5;
            }
        }
    }

    map
}

pub struct LemmaDictionary;

impl LemmaDictionary {
    pub fn new() -> Self {
        STOPWORDS_SET.get_or_init(|| ENGLISH_STOPWORDS.iter().copied().collect());
        VOCAB_MAP.get_or_init(parse_builtin_and_ground_truth_vocab);
        Self
    }

    /// Returns the total number of distinct lemmas loaded in the dictionary.
    pub fn len(&self) -> usize {
        let vocab = VOCAB_MAP.get_or_init(parse_builtin_and_ground_truth_vocab);
        vocab.len()
    }

    /// Returns whether the dictionary contains no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Looks up a word candidate in the dictionary.
    /// Normalizes punctuation and attempts standard English suffix stemming.
    pub fn lookup(&self, word: &str, min_word_len: usize) -> Option<(u8, u32)> {
        let stopwords = STOPWORDS_SET.get_or_init(|| ENGLISH_STOPWORDS.iter().copied().collect());
        let vocab = VOCAB_MAP.get_or_init(parse_builtin_and_ground_truth_vocab);

        let mut clean = word.trim().trim_matches(&['\'', '’', '"', '“', '”', '`', ',', '.', ';', ':', '!', '?', '-'][..]).to_lowercase();
        if clean.contains('’') {
            clean = clean.replace('’', "'");
        }
        if let Some(stripped) = clean.strip_suffix("'s") {
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
                if stem.len() >= min_word_len
                    && let Some(&res) = vocab.get(stem) {
                        return Some(res);
                    }
                // Try with trailing 'e' (e.g. "ameliorating" -> "ameliorat" -> "ameliorate")
                let with_e = format!("{}e", stem);
                if with_e.len() >= min_word_len
                    && let Some(&res) = vocab.get(with_e.as_str()) {
                        return Some(res);
                    }
                // Doubled consonant (e.g. "compelled" -> "compel")
                if stem.len() > 2 {
                    let bytes = stem.as_bytes();
                    if bytes[bytes.len() - 1] == bytes[bytes.len() - 2] {
                        let single = &stem[..stem.len() - 1];
                        if single.len() >= min_word_len
                            && let Some(&res) = vocab.get(single) {
                                return Some(res);
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

        // 1. Direct hit (Difficulty 1 GRE / CEFR C2)
        let res = dict.lookup("ameliorate", 3);
        assert_eq!(res, Some((1, 35386)));

        // 2. Inflected form (-ing with added 'e')
        let res2 = dict.lookup("ameliorating", 3);
        assert_eq!(res2, Some((1, 35386)));

        // 3. Stopword filtered (including curly apostrophe normalization)
        assert_eq!(dict.lookup("the", 3), None);
        assert_eq!(dict.lookup("because", 3), None);
        assert_eq!(dict.lookup("don't", 3), None);
        assert_eq!(dict.lookup("don’t", 3), None);
        assert_eq!(dict.lookup("wasn’t", 3), None);
        assert_eq!(dict.lookup("--the--", 3), None);

        // 4. Short word filtered
        assert_eq!(dict.lookup("it", 3), None);

        // 5. Doubled consonant
        let res3 = dict.lookup("compelled", 3);
        assert_eq!(res3, Some((3, 1286)));

        // 6. Smart apostrophe (UTF-8 3-byte curly quote) - must not panic
        let res4 = dict.lookup("ameliorate’s", 3);
        assert_eq!(res4, Some((1, 35386)));
        let res5 = dict.lookup("ameliorate's", 3);
        assert_eq!(res5, Some((1, 35386)));

        // 7. Verify curated difficulty calibrations for jaundiced and housewife
        assert_eq!(dict.lookup("jaundiced", 3), Some((1, 19346)));
        assert_eq!(dict.lookup("jaundice", 3), Some((1, 1306)));
        assert_eq!(dict.lookup("housewife", 3), Some((5, 7958)));
    }

    #[test]
    fn test_ground_truth_vocabulary_scale_and_difficulties() {
        let dict = LemmaDictionary::new();

        // Ensure 20,000+ entries loaded
        assert!(
            dict.len() >= 20_000,
            "Expected at least 20,000 ground truth lemmas, got {}",
            dict.len()
        );
        assert!(!dict.is_empty());

        // Test lookups across all 5 difficulty levels from ground truth dataset
        let diff1 = dict.lookup("abstruse", 3);
        assert!(diff1.is_some());
        assert_eq!(diff1.unwrap().0, 1);

        let diff2 = dict.lookup("zenith", 3);
        assert!(diff2.is_some());
        assert_eq!(diff2.unwrap().0, 2);

        let diff3 = dict.lookup("plausibility", 3);
        assert!(diff3.is_some());
        assert_eq!(diff3.unwrap().0, 3);

        let diff4 = dict.lookup("unearth", 3);
        assert!(diff4.is_some());
        assert_eq!(diff4.unwrap().0, 4);

        let diff5 = dict.lookup("handbook", 3);
        assert!(diff5.is_some());
        assert_eq!(diff5.unwrap().0, 5);
    }
}
