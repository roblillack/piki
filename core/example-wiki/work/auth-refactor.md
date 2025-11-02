# Authentication Refactor

**Sprint Window**: Oct 21 – Nov 1  
**Objective**: Migrate to new token service with refresh token rotation

## Status Overview

- Backend service deployed behind feature flag in staging
- Integration tests passing except for legacy mobile client flow
- Documentation draft ready for review (`work/api-documentation-update`)

## Work Breakdown

1. **Token Issuance Module**
   - [x] Swap JWT library to internal crate
   - [x] Add rotatable refresh tokens + revocation list
2. **Client SDK Updates**
   - [ ] Publish TypeScript client beta (blocked on staging cert issue)
   - [ ] Update Python SDK with new error handling codes
3. **Rollout Plan**
   - [ ] Announce timeline to customer success (template drafted)
   - [ ] Coordinate staged rollout with SRE (include rollback steps)

## Open Questions

- How do we migrate service accounts that cannot store refresh tokens?
- Do we have metrics for failed refresh attempts in Datadog dashboards?
- Confirm SOC2 implications with security (email sent 10/26)
