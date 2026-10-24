# Specification: Pre-flight Reference Validator for Yellowjacket KFX

## 1. Objectives & Overview
The purpose of the **Pre-flight Reference Validator** is to guarantee that every KFX container produced by `converter_epub2kfx` has 100% referential integrity before it is written to disk or transferred to a Kindle device.
This completely prevents the fatal `Referenced fragment is missing from book` error and the associated native `libYJSDK` SIGSEGV crashes.

## 2. Referential Integrity Rules
Based on the Amazon Yellowjacket specification and Calibre `kfxlib` fragment graph checks, the validator checks the following rules:

| Source Field ID | Field Name | Target Chunk Type | Description |
| :--- | :--- | :--- | :--- |
| **`$392`** | `nav_containers` | **`$391`** (`nav_container`) | Every symbol in navigation reading order must exist as a standalone nav container. |
| **`$170`** | `sections` | **`$260`** (`section`) | Every section symbol in reading orders must exist as a section template. |
| **`$174`** | `section_name` | **`$260`** (`section`) | Every section reference in navigation, maps, and templates must exist. |
| **`$176`** | `story_name` | **`$259`** (`storyline`) | Storyline references in sections and maps must exist. |
| **`$175`** | `resource_name` | **`$164`** (`external_resource`) | Resource symbols referenced in content or metadata must exist. |
| **`$165`** | `location` | **`$417`** (`bcRawMedia`) | External resources must point to an existing raw media payload entity. |
| **`$173`** | `style_name` | **`$157`** (`style`) | Style names in styles must match their entity IDs. |
| **`$429`** | `style` | **`$157`** (`style`) | Styles applied in storylines and spans must exist as style entities. |
| **`$245`** | `icon` | **`$164`** (`external_resource`) | Icons in navigation or content must exist as external resources. |
| **`$254`** | `mandatory_dependencies` | **Any** | Dependencies in `$419` must exist in the container. |
| **`$181`** | `contains` | **Any non-singleton** | All non-singleton entities in the container must be in `contains`, with no phantom symbols. |
| **`$4`** (Forbidden in Styles) | `name` | **`$145`** | If `name` ($4) is present in a style struct (`$157`), the validator rejects it because Calibre/Yellowjacket treats it as a missing content entity reference. |

## 3. Architecture & Implementation Plan
1. **`src/ion.rs` - `IonReader`**:
   - Provide a safe binary Ion decoder `IonReader::decode_value(&[u8]) -> Result<IonValue, IonError>` to deserialize binary Ion payloads (BVM + descriptors) into in-memory `IonValue` trees.
2. **`src/validator.rs` - `KfxPreflightValidator`**:
   - Register all container entities: `Map<(chunk_type, entity_id), &KfxEntity>`.
   - Parse and walk each entity's Ion tree.
   - For every field matching the reference rules, verify target entity existence.
   - Return `ValidationReport` or `ValidationError::MissingFragment`.
3. **Integration in `src/packager.rs`**:
   - Call `KfxPreflightValidator::validate(&self.entities, &symbols)` inside `package()` before serialization.
4. **Comprehensive Test Suite**:
   - Deterministic unit tests covering positive (valid book) and negative (missing nav, missing style, missing resource, illegal symbol) cases.
