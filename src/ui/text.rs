//! Player-facing display strings.
//!
//! Kept in one place so they can move into a localization table later.
//! Systems must use these instead of inline string literals.

pub const SPLASH_TITLE: &str = "ROADSIDE PICNIC";
pub const SPLASH_LOADING: &str = "Loading the Zone...";
pub const SPLASH_READY: &str = "Press any key to continue";
pub const SPLASH_FAILED: &str = "Could not load the Zone. Close the game and try again.";
pub const ENTERING_ZONE: &str = "Entering the Zone...";

/// Title shown in the HUD corner.
pub const HUD_TITLE: &str = "THE ZONE";
pub const HUD_FPS_PENDING: &str = "FPS: --";
pub const HUD_POSITION_PENDING: &str = "X: -- Y: -- Z: --";

/// Shown for a moment after progress is saved (F5, every minute, on quitting).
pub const PROGRESS_SAVED: &str = "Progress saved";

/// Hint shown in the HUD corner.
pub const HUD_CONTROLS_HINT: &str = "Click: move | Shift+Click: waypoint | WASD: steer | Wheel: zoom | Arrows/Edges/Middle-drag: pan | F: free camera | C: find stalker | M: map | F5: save | Hold T: time-lapse | N: mute | ESC: pause | Q: quit";
pub const HUD_CONTROLS_MUTED: &str = "Click: move | Shift+Click: waypoint | WASD: steer | Wheel: zoom | Arrows/Edges/Middle-drag: pan | F: free camera | C: find stalker | M: map | F5: save | Hold T: time-lapse | N: unmute | ESC: pause | Q: quit";
pub const HUD_DRIVER_CONTROLS: &str = "Click: drive | Shift+Click: waypoint | W/S: accelerate/reverse | A/D: steer | SPACE: brake | F: free camera | C: find stalker | M: map | F5: save | N: mute";
pub const HUD_DRIVER_MUTED: &str = "Click: drive | Shift+Click: waypoint | W/S: accelerate/reverse | A/D: steer | SPACE: brake | F: free camera | C: find stalker | M: map | F5: save | N: unmute";
/// Shown while the view roams away from the stalker.
pub const HUD_FREE_CAMERA: &str = "FREE CAMERA - F or C: follow the stalker";
pub const HUD_ENTER_VEHICLE: &str = "Press E to enter vehicle";
pub const HUD_EXIT_VEHICLE: &str = "Press E to exit vehicle";

/// A described thing: its name and a line or two about it.
pub struct Entry {
    pub title: &'static str,
    pub description: &'static str,
}

pub const ARTIFACT_READOUT_LABEL: &str = "RECOVERED ARTIFACT";

pub const ARTIFACT_COPY: [Entry; 14] = [
    Entry {
        title: "Death Lamp",
        description: "A cylinder that emits invisible radiation lethal to living things within its range.",
    },
    Entry {
        title: "Golden Sphere / Golden Ball",
        description: "A heavy, warm sphere said to reveal a person's deepest true desire and grant it, with a human sacrifice required to bypass its guardian anomaly.",
    },
    Entry {
        title: "Itcher",
        description: "Proximity can cause severe distress, panic, aggression, and uncontrollable barking or howling in animals and humans.",
    },
    Entry {
        title: "Lobster Eyes",
        description: "Glassy, iridescent spheres whose optical properties remain unexplained.",
    },
    Entry {
        title: "Spinning Ring / Perpetual Ring",
        description: "A white metal ring that spins endlessly on a flat surface without visible friction or energy loss.",
    },
    Entry {
        title: "Sponge",
        description: "A porous object believed to absorb and neutralize certain Zone toxins or radiation.",
    },
    Entry {
        title: "White Ring / Rattling Napkin",
        description: "A strange, non-reactive curiosity; no reliable practical use has been discovered.",
    },
    Entry {
        title: "Wriggling Magnet",
        description: "A magnetic object that vibrates and oscillates continuously despite having no visible power source.",
    },
    Entry {
        title: "Black Spark / Black Spray",
        description: "A black bead or droplet that absorbs and delays light, making movement appear visually out of step.",
    },
    Entry {
        title: "Bracelet",
        description: "A metallic ring that emits radiation or stimulates biological processes, affecting health and metabolism.",
    },
    Entry {
        title: "Empty / Spacer",
        description: "Two copper discs held about eighteen inches apart; the empty gap resists forces trying to close or separate them.",
    },
    Entry {
        title: "Full Empty",
        description: "An Empty containing luminous blue fluid within the gap. It is rarer, heavier, and less understood than a standard Empty.",
    },
    Entry {
        title: "Pins & Needles",
        description: "Small crystalline or metallic objects that emit faint colored flashes or magnetic pulses; their purpose is unknown.",
    },
    Entry {
        title: "So-So / Etak",
        description: "Small black cylinders or discs called batteries. They generate endless electricity and can self-replicate under certain conditions.",
    },
];

/// Hover descriptions: kind labels.
pub const KIND_LANDMARK: &str = "Landmark";
pub const KIND_SITE: &str = "Site";
pub const KIND_BUILDING: &str = "Building";
pub const KIND_SWAMP: &str = "Swamp";
pub const KIND_LAKE: &str = "Lake";
pub const KIND_COAST: &str = "Coast";
pub const KIND_WILDLIFE: &str = "Wildlife";
pub const KIND_MUTANT: &str = "Mutant";
pub const KIND_VEHICLE: &str = "Vehicle";
pub const KIND_GUARD: &str = "Cordon guard";
pub const KIND_PASSAGE: &str = "Hidden passage";
pub const KIND_ARTIFACT: &str = "Recovered artifact";

pub const UNEXPLORED_DESCRIPTION: &str =
    "Not surveyed yet. Get within 150 m on foot or by car to learn what it is.";

pub fn unexplored(kind: &str) -> String {
    format!("Unexplored {}", kind.to_lowercase())
}

pub const TOWN: Entry = Entry {
    title: "New Harmont",
    description: "The clean districts around the old square, where the evacuated and the UN staff live as far from the Zone as the town allows.",
};

/// Indexed like the world's site kinds.
pub const SITE_COPY: [Entry; 24] = [
    Entry {
        title: "International Institute",
        description: "Thirteen storeys on the Perimeter itself. From its leaded top-floor windows you can see almost the whole Zone.",
    },
    Entry {
        title: "The Borscht",
        description: "A sagging two-storey tavern at the edge of town where stalkers drink. The back room buys what came out of the Zone last night, and the police know it.",
    },
    Entry {
        title: "The Old Borscht",
        description: "Ernest's first bar, gutted in a police raid. He reopened down the lane; the broken sign still hangs over the door.",
    },
    Entry {
        title: "Plague Quarter",
        description: "The streets nearest the Zone, abandoned by all but a few holdouts. The dead have been known to come home here.",
    },
    Entry {
        title: "Evacuated house",
        description: "Left the day of the Visitation, with the table still laid. Nobody came back for the furniture.",
    },
    Entry {
        title: "Schoolhouse",
        description: "Desks still in rows, and a clock stopped at the hour of the Visitation.",
    },
    Entry {
        title: "Cemetery",
        description: "A toppled cross over disturbed earth. Locals say not everyone here stayed buried.",
    },
    Entry {
        title: "Abandoned Suburbs",
        description: "Crumbling houses and overgrown yards just inside the Perimeter. Stalkers crawl through the drainage ditches; mosquito balds wait in the lanes.",
    },
    Entry {
        title: "Old Factory",
        description: "Rusted machinery and trenches on the near side of the Zone. Hell slime pools in the pits, and the ground is heavy in places.",
    },
    Entry {
        title: "Brick factory",
        description: "Gutted kilns and a collapsed drying hall under a chimney wrapped in dangling cable.",
    },
    Entry {
        title: "Grain silos",
        description: "Concrete silos whose grain turned to fine grey ash overnight.",
    },
    Entry {
        title: "Substation",
        description: "Dead transformers that sometimes spark, though the lines carry no current.",
    },
    Entry {
        title: "Abandoned substation",
        description: "A switchyard of rusted transformers and sagging lines. Compasses spin near its busbars.",
    },
    Entry {
        title: "Broken Asphalt Highway",
        description: "The cracked main road from the Institute gate into the middle of the Zone. The air shimmers over it like heat, even in winter.",
    },
    Entry {
        title: "The Garage",
        description: "A ruined transport depot halfway in. Red used it as a staging point; silver cobweb hangs in its corners.",
    },
    Entry {
        title: "Motor pool",
        description: "A repair shop abandoned mid-job; tools still lie where the mechanics dropped them.",
    },
    Entry {
        title: "Railway embankment",
        description: "An overgrown rail bed, raised above the anomalies. Stalkers follow it to slip past the worst of them.",
    },
    Entry {
        title: "Locomotive",
        description: "An engine stopped where its crew left it. The rails hum before a storm.",
    },
    Entry {
        title: "The Marsh",
        description: "Treacherous wetland deep inside. Toss a nut before every step: witches' jelly lies in the pools.",
    },
    Entry {
        title: "Witches' jelly flats",
        description: "Grey mud that eats through boots and bone alike. Nothing grows here but dead trunks.",
    },
    Entry {
        title: "The Ridge",
        description: "Bare rocky hills beyond the marsh, overlooking the quarry basin. Jolly ghosts spin in the hollows.",
    },
    Entry {
        title: "Crater amphitheater",
        description: "A bowl of glassy slag pressed into the earth, terraced like seats around a stage no one built.",
    },
    Entry {
        title: "Quarry rim",
        description: "The narrow way down into the pit. The Meat Grinder waits at the bottleneck; someone always goes first.",
    },
    Entry {
        title: "The Golden Sphere",
        description: "At the bottom of the old quarry: the wish machine. Happiness for everybody, free, and no one will go away unsatisfied.",
    },
];

pub const DERAILED_TRAIN: Entry = Entry {
    title: "Derailed train",
    description: "A freight train thrown off the embankment. Its cargo was never claimed.",
};
pub const HANGAR: Entry = Entry {
    title: "Institute hangar",
    description: "Base camp on the Harmont side: the workshop, the map board, the boot and the recovered artifacts. The Zone's creatures stay out.",
};
pub const CHECKPOINT: Entry = Entry {
    title: "UN checkpoint",
    description: "Sandbags, a turnstile and a watchtower. Passes are checked here on the way to the Perimeter gate; beyond it, the Zone begins.",
};
pub const YARD: Entry = Entry {
    title: "Hangar yard",
    description: "Walled, level staging ground around the hangar, kept clear for the car.",
};
pub const QUARRY: Entry = Entry {
    title: "The Quarry",
    description: "The vast excavation at the far end of the Zone, with the Golden Sphere at the bottom of its deepest pit.",
};
pub const BALD_HILL: Entry = Entry {
    title: "Bald hill",
    description: "A scorched mound where nothing has grown since the Visitation.",
};
pub const STAGNANT_FLATS: Entry = Entry {
    title: "Stagnant flats",
    description: "A sunken, waterlogged lowland. Fog settles here first.",
};
pub const SWAMP: Entry = Entry {
    title: "Swamp",
    description: "Shallow pools between mossy hummocks. Wade with care: the mud keeps what it takes.",
};
pub const LAKE: Entry = Entry {
    title: "Lake",
    description: "A broad basin lake beyond the quarry, past the edge of the map.",
};
pub const COAST: Entry = Entry {
    title: "Coast",
    description: "The grey southern sea at the edge of the Zone.",
};
pub const VEHICLE: Entry = Entry {
    title: "Armored vehicle",
    description: "The stalkers' armored car. Press E beside it to get in, then click to drive.",
};
pub const GUARD: Entry = Entry {
    title: "UN soldier",
    description: "One of the international cordon's sentries, pacing beside the opening and watching who comes through.",
};
pub const PATROL: Entry = Entry {
    title: "UN patrol car",
    description: "A white armored car of the international forces, driving the length of the Perimeter wall on the Harmont side.",
};
pub const SWEEPER: Entry = Entry {
    title: "Street sweeper",
    description: "Harmont's sweeper, brushing the dust off New Harmont's avenue. Nobody in town asks where the dust comes from.",
};
pub const SEDAN: Entry = Entry {
    title: "Harmont sedan",
    description: "Townsfolk about their business between New Harmont, the Institute and the Borscht.",
};
pub const CRAWL_HOLE: Entry = Entry {
    title: "Crawl hole",
    description: "Someone has dug under the Perimeter here and let the weeds grow over it. Lie flat (hold Ctrl) to crawl under the wall.",
};

/// Indexed like the world's creature kinds.
pub const CREATURE_COPY: [Entry; 13] = [
    Entry {
        title: "Frostwolf",
        description: "A pale wolf whose breath fogs even in summer. It hunts the hexadeer near the hangar.",
    },
    Entry {
        title: "Hexadeer",
        description: "A six-legged deer, skittish and fast.",
    },
    Entry {
        title: "Skyglider",
        description: "A broad-winged glider that rides the thermals over the Zone.",
    },
    Entry {
        title: "Venom chameleon",
        description: "It clings to its perch and takes on the colors of the light. Do not touch.",
    },
    Entry {
        title: "Hawk",
        description: "Circles high above the open ground.",
    },
    Entry {
        title: "Heron",
        description: "Wades the shallows of the river and the lake.",
    },
    Entry {
        title: "Owl",
        description: "A night hunter; its eyes catch the moonlight.",
    },
    Entry {
        title: "Raven",
        description: "Lands, struts and takes off around the hangar. Clever and watchful.",
    },
    Entry {
        title: "Sparrow",
        description: "Small flocks dart between the bushes.",
    },
    Entry {
        title: "Mutant bovine",
        description: "A hulking grazer. It gives way unless cornered.",
    },
    Entry {
        title: "Chimera",
        description: "A predator stitched from several animals. It stalks, braces and lunges.",
    },
    Entry {
        title: "Fleshdeer",
        description: "Curious and unsettling: it comes close to look at you.",
    },
    Entry {
        title: "Mutt",
        description: "A feral mutant dog. It chases, so keep the car close.",
    },
];

/// Title shown in the pause menu.
pub const PAUSE_TITLE: &str = "PAUSED";
pub const MAP_TITLE: &str = "HARMONT AND THE ZONE";
pub const MAP_HINT: &str = "M: close the map";
/// A bird on the map, its wings up and down.
pub const MAP_BIRD: [&str; 2] = ["v", "-"];

/// Hint shown in the pause menu.
pub const PAUSE_HINT: &str = "ESC: resume";

/// Formats the per-run Zone statistics line for the HUD.
pub fn hud_zone_stats(artifacts: u32, anomalies: u32) -> String {
    format!("Artifacts: {artifacts} | Anomalies triggered: {anomalies}")
}

pub const SKY_CLEAR: &str = "Clear";
pub const SKY_HUMID: &str = "Humid";
pub const SKY_OVERCAST: &str = "Overcast";
pub const SKY_RAIN: &str = "Rain";

/// Formats the clock and weather line, e.g. `06:15 | 9 C | 97% humidity | Humid`.
pub fn hud_weather(hour: f32, temperature: f32, humidity: f32, sky: &str) -> String {
    let minutes = (hour * 60.0) as u32 % (24 * 60);
    format!(
        "{:02}:{:02} | {temperature:.0} C | {:.0}% humidity | {sky}",
        minutes / 60,
        minutes % 60,
        humidity * 100.0
    )
}

pub fn hud_fps(fps: f64) -> String {
    format!("FPS: {fps:.0}")
}

/// The stalker's world position in meters (Y is up).
pub fn hud_position(at: [f32; 3]) -> String {
    let [x, y, z] = at;
    format!("X: {x:.1} Y: {y:.1} Z: {z:.1}")
}
