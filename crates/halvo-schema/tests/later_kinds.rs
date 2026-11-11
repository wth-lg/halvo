use halvo_schema::kinds::{
    check_entries, check_record, FieldType, KindEntry, Origin, RetentionClass, KINDS,
};

use FieldType::{App, Count, Day, Hash, Id, Instant, Name, OwnText, Site, Version};

const AGENT_CONFIG: KindEntry = KindEntry {
    name: "agent_config",
    class: RetentionClass::Kept,
    origin: Origin::Capture,
    family: "agent",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", Version),
        ("workflow", Id),
        ("agent", Id),
        ("rev", Count),
        ("category", Name),
        ("tier", Name),
        ("reasoning", Name),
        ("autonomy", Name),
        ("allow", Name),
        ("caps", Name),
        ("limits", Count),
        ("budget", Count),
        ("budget", Name),
        ("permissions", Name),
        ("runtime", Name),
        ("playbook", Count),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","rev":1,"category":"utility_bill_payment","tier":"on_device","reasoning":"low","autonomy":"draft_and_confirm","allow":[],"caps":{"untrusted":"yes","private":"yes","external":"no"},"limits":{"steps":40,"wall_s":600,"model_tokens":20000},"budget":null,"permissions":[],"runtime":"web","playbook":null}"#,
};

const AGENT_RUN: KindEntry = KindEntry {
    name: "agent_run",
    class: RetentionClass::Bounded { days: 93 },
    origin: Origin::Capture,
    family: "agent",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", Version),
        ("workflow", Id),
        ("agent", Id),
        ("run", Id),
        ("day", Day),
        ("mode", Name),
        ("outcome", Name),
        ("playbook", Count),
        ("steps", Count),
        ("prompts", Count),
        ("handbacks", Count),
        ("confirmations", Count),
        ("effects", Count),
        ("unknown", Count),
        ("model_calls", Count),
        ("tokens_in", Count),
        ("tokens_out", Count),
        ("wall_s", Count),
        ("model", Name),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","run":"40000000000000000000000000000001","day":20767,"mode":"replay","outcome":"done","playbook":1,"steps":12,"prompts":0,"handbacks":1,"confirmations":1,"effects":1,"unknown":0,"model_calls":3,"tokens_in":5000,"tokens_out":400,"wall_s":95,"model":"local_small"}"#,
};

const AGENT_PLAYBOOK: KindEntry = KindEntry {
    name: "agent_playbook",
    class: RetentionClass::Kept,
    origin: Origin::Own,
    family: "agent",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", Version),
        ("workflow", Id),
        ("agent", Id),
        ("number", Count),
        ("n", Count),
        ("step", Count),
        ("step", Name),
        ("step", OwnText),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","number":1,"n":4,"step":{"v":1,"n":4,"kind":"web","verb":"click","target":{"desc":"button Submit payment","frame":["https://shop.test:443"],"credential":"no","locators":[{"by":"role","value":"button","name":"Submit payment"},{"by":"test_id","value":"pay","name":null}]},"args":[],"pre":[{"check":"present","target":null,"arg":null}],"post":[],"effect":"request","class":"submit"}}"#,
};

const AGENT_BINDINGS: KindEntry = KindEntry {
    name: "agent_bindings",
    class: RetentionClass::Kept,
    origin: Origin::Own,
    family: "agent",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", Version),
        ("workflow", Id),
        ("agent", Id),
        ("rev", Count),
        ("origins", OwnText),
        ("slots", Name),
        ("slots", OwnText),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","rev":2,"origins":["https://shop.test:443"],"slots":[{"name":"account","value":["Example Power account 7"]}]}"#,
};

const AGENT_EFFECT: KindEntry = KindEntry {
    name: "agent_effect",
    class: RetentionClass::Bounded { days: 93 },
    origin: Origin::Own,
    family: "agent",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", Version),
        ("workflow", Id),
        ("agent", Id),
        ("run", Id),
        ("effect", Id),
        ("seq", Count),
        ("day", Day),
        ("at", Instant),
        ("state", Name),
        ("class", Name),
        ("digest", Hash),
        ("kept", Name),
        ("kept", OwnText),
        ("fields", Count),
        ("approval", Id),
        ("cause", Name),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","run":"40000000000000000000000000000001","effect":"50000000000000000000000000000001","seq":2,"day":20767,"at":1794339000000000000,"state":"released","class":"submit","digest":"abababababababababababababababababababababababababababababababab","kept":[{"name":"amount","value":["42.10"]}],"fields":3,"approval":"60000000000000000000000000000001","cause":null}"#,
};

const AGENT_LOG: KindEntry = KindEntry {
    name: "agent_log",
    class: RetentionClass::Bounded { days: 93 },
    origin: Origin::Own,
    family: "agent",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", Version),
        ("workflow", Id),
        ("agent", Id),
        ("run", Id),
        ("seq", Count),
        ("day", Day),
        ("at", Instant),
        ("event", Name),
        ("step", Count),
        ("cause", Name),
        ("model", Name),
        ("ms", Count),
        ("tokens_in", Count),
        ("tokens_out", Count),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","run":"40000000000000000000000000000001","seq":7,"day":20767,"at":1794339000000000000,"event":"model_call","step":4,"cause":null,"model":"local_small","ms":850,"tokens_in":1200,"tokens_out":80}"#,
};

const AGENT_SESSION: KindEntry = KindEntry {
    name: "agent_session",
    class: RetentionClass::Bounded { days: 35 },
    origin: Origin::Own,
    family: "agent",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", Version),
        ("workflow", Id),
        ("agent", Id),
        ("origin", OwnText),
        ("rev", Count),
        ("part", Count),
        ("of", Count),
        ("day", Day),
        ("data", OwnText),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","origin":"https://shop.test:443","rev":1,"part":1,"of":1,"day":20767,"data":["c2Vzc2lvbg=="]}"#,
};

const COORD_EVENT: KindEntry = KindEntry {
    name: "coord_event",
    class: RetentionClass::Bounded { days: 93 },
    origin: Origin::Own,
    family: "coord",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", Version),
        ("workflow", Id),
        ("agent", Id),
        ("seq", Count),
        ("day", Day),
        ("at", Instant),
        ("event", Name),
        ("run", Id),
        ("slot", Day),
        ("item", Id),
        ("kind", Name),
        ("effect", Id),
        ("approval", Id),
        ("digest", Hash),
        ("answer", Name),
        ("cause", Name),
        ("trigger", Name),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","seq":5,"day":20767,"at":1794339000000000000,"event":"item_answered","run":"40000000000000000000000000000001","slot":null,"item":"50000000000000000000000000000002","kind":"confirmation","effect":"50000000000000000000000000000001","approval":"60000000000000000000000000000001","digest":"abababababababababababababababababababababababababababababababab","answer":"allow_once","cause":null,"trigger":null}"#,
};

const LEARN_RUN: KindEntry = KindEntry {
    name: "learn_run",
    class: RetentionClass::Bounded { days: 93 },
    origin: Origin::Capture,
    family: "learn",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", Version),
        ("workflow", Id),
        ("agent", Id),
        ("run", Id),
        ("rev", Count),
        ("day", Day),
        ("mode", Name),
        ("state", Name),
        ("cause", Name),
        ("base", Count),
        ("assisted", Count),
        ("kept", Count),
        ("failed_step", Count),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","run":"40000000000000000000000000000001","rev":2,"day":20767,"mode":"replay","state":"verified","cause":null,"base":1,"assisted":1,"kept":1,"failed_step":null}"#,
};

const LEARN_EDIT: KindEntry = KindEntry {
    name: "learn_edit",
    class: RetentionClass::Kept,
    origin: Origin::Own,
    family: "learn",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", Version),
        ("workflow", Id),
        ("agent", Id),
        ("edit", Id),
        ("rev", Count),
        ("base", Count),
        ("n", Count),
        ("state", Name),
        ("cause", Name),
        ("on", Id),
        ("runs", Id),
        ("first", Day),
        ("last", Day),
        ("clean", Count),
        ("target", OwnText),
        ("target", Name),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","edit":"50000000000000000000000000000003","rev":1,"base":1,"n":4,"state":"provisional","cause":null,"on":null,"runs":["40000000000000000000000000000001"],"first":20767,"last":20767,"clean":0,"target":{"desc":"button \"Submit payment\"","locators":[{"by":"role","value":"button","name":"Submit payment"}]}}"#,
};

const LEARN_ROUTINE: KindEntry = KindEntry {
    name: "learn_routine",
    class: RetentionClass::Bounded { days: 93 },
    origin: Origin::Capture,
    family: "learn",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", Version),
        ("workflow", Id),
        ("agent", Id),
        ("rev", Count),
        ("day", Day),
        ("category", Name),
        ("site", Site),
        ("app", App),
        ("period", Count),
        ("seen", Count),
        ("first", Day),
        ("last", Day),
        ("next", Day),
        ("missed", Count),
        ("mark", Name),
        ("since", Day),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","rev":3,"day":20767,"category":"utility_bill_payment","site":{"key":"shop.test","source":"address_field"},"app":"com.example.browser","period":30,"seen":3,"first":20705,"last":20765,"next":20795,"missed":0,"mark":"none","since":null}"#,
};

const LATER: [&KindEntry; 11] = [
    &AGENT_CONFIG,
    &AGENT_RUN,
    &AGENT_PLAYBOOK,
    &AGENT_BINDINGS,
    &AGENT_EFFECT,
    &AGENT_LOG,
    &AGENT_SESSION,
    &COORD_EVENT,
    &LEARN_RUN,
    &LEARN_EDIT,
    &LEARN_ROUTINE,
];

const OTHER_STATES: [(&str, &str); 11] = [
    (
        "agent_config",
        r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","rev":4,"category":"shop_order","tier":"frontier","reasoning":"high","autonomy":"unattended","allow":["send","submit"],"caps":{"untrusted":"no","private":"yes","external":"yes"},"limits":{"steps":0,"wall_s":0,"model_tokens":0},"budget":{"amount":500,"period":"month"},"permissions":["user_files"],"runtime":"graph_mail","playbook":3}"#,
    ),
    (
        "agent_run",
        r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","run":"40000000000000000000000000000002","day":20768,"mode":"compile","outcome":"handed_back","playbook":null,"steps":0,"prompts":2,"handbacks":0,"confirmations":0,"effects":0,"unknown":1,"model_calls":0,"tokens_in":0,"tokens_out":0,"wall_s":4,"model":null}"#,
    ),
    (
        "agent_playbook",
        r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","number":2,"n":1,"step":{"v":1,"n":1,"kind":"api","verb":"send","target":{"adapter":"graph_mail","op":"send"},"args":[{"name":"text","from":"slot","value":null,"key":"amount"},{"name":"to","from":"const","value":"Accounts payable","key":null}],"pre":[],"post":[{"check":"count_is","target":null,"arg":{"name":"count","from":"const","value":"1","key":null}}],"effect":"none","class":"send"}}"#,
    ),
    (
        "agent_bindings",
        r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","rev":1,"origins":[],"slots":[]}"#,
    ),
    (
        "agent_effect",
        r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","run":"40000000000000000000000000000001","effect":"50000000000000000000000000000001","seq":3,"day":20767,"at":-1,"state":"lost","class":"submit","digest":"abababababababababababababababababababababababababababababababab","kept":[],"fields":0,"approval":null,"cause":"declined"}"#,
    ),
    (
        "agent_log",
        r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","run":"40000000000000000000000000000001","seq":8,"day":20767,"at":1794339000000000000,"event":"stopped","step":null,"cause":"session_size","model":null,"ms":null,"tokens_in":null,"tokens_out":null}"#,
    ),
    (
        "agent_session",
        r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","origin":"https://mail.shop.test:8443","rev":2,"part":2,"of":2,"day":20768,"data":["AAAA","BBBB"]}"#,
    ),
    (
        "coord_event",
        r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","seq":1,"day":20767,"at":1794339000000000000,"event":"run_started","run":"40000000000000000000000000000001","slot":20767,"item":null,"kind":null,"effect":null,"approval":null,"digest":null,"answer":null,"cause":null,"trigger":"schedule"}"#,
    ),
    (
        "learn_run",
        r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","run":"40000000000000000000000000000001","rev":1,"day":20767,"mode":"compile","state":"started","cause":null,"base":null,"assisted":0,"kept":0,"failed_step":null}"#,
    ),
    (
        "learn_edit",
        r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","edit":"50000000000000000000000000000003","rev":3,"base":1,"n":4,"state":"reverted","cause":"step_failed","on":"50000000000000000000000000000004","runs":["40000000000000000000000000000001","40000000000000000000000000000002","40000000000000000000000000000003"],"first":20767,"last":20790,"clean":2,"target":{"desc":"link Pay now","locators":[{"by":"css","value":"a.pay","name":null},{"by":"label","value":"Pay now","name":null}]}}"#,
    ),
    (
        "learn_routine",
        r#"{"v":1,"workflow":"10000000000000000000000000000001","agent":"30000000000000000000000000000001","rev":1,"day":20767,"category":"spreadsheet_fill","site":null,"app":"com.example.sheets","period":null,"seen":0,"first":null,"last":null,"next":null,"missed":0,"mark":"stale","since":20760}"#,
    ),
];

#[test]
fn the_kinds_of_the_agent_setup_interface_and_learning_runs_give_no_report_beside_the_four_of_this_run(
) {
    let all: Vec<&KindEntry> = KINDS.iter().chain(LATER).collect();
    assert_eq!(all.len(), 15);
    assert_eq!(check_entries(&all), Vec::<String>::new());
    for entry in LATER {
        assert_eq!(
            check_entries(&[entry]),
            Vec::<String>::new(),
            "{}",
            entry.name
        );
    }
}

#[test]
fn a_row_of_each_kind_of_the_later_runs_passes_check_record_in_each_state_of_its_contract() {
    for entry in LATER {
        assert_eq!(check_record(entry, entry.example), Ok(()), "{}", entry.name);
    }
    for (name, row) in OTHER_STATES {
        let entry = LATER.iter().find(|e| e.name == name).expect(name);
        assert_eq!(check_record(entry, row), Ok(()), "{name}");
    }
}
