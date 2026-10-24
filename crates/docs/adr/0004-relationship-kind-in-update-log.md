# Preserve relationship kind in the Update Log

Forge will record the Relationship kind on every newly written relationship-addition or -removal Record Update and expose it through RPC and subscription delivery. This makes a historical change unambiguous once a Record pair can have more than one Relationship kind; legacy updates retain an absent kind because it cannot be reconstructed truthfully after a removed relationship.
