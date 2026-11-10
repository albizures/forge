# Preserve relationship kind in the Update Log

Current scope: subscription delivery has been removed. Preserving Relationship
kind in Record Updates and exposing it through read RPCs remains unchanged.
The decision below records the original scope.

Forge will record the Relationship kind on every newly written relationship-addition or -removal Record Update and expose it through RPC and subscription delivery. This makes a historical change unambiguous once a Record pair can have more than one Relationship kind; legacy updates retain an absent kind because it cannot be reconstructed truthfully after a removed relationship.
