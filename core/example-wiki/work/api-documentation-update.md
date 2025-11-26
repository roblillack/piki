# API Documentation Update

**Scope**: Align developer docs with authentication changes and new rate limits.

## Sections to Update

- **Authentication**  
  - Document refresh token rotation  
  - Add troubleshooting for `invalid_refresh_token` errors  
  - Include migration guide for service accounts
- **Rate Limiting**  
  - Update limits to 10k/hour for enterprise tier  
  - Add sample error payload for throttling responses
- **Webhooks**  
  - Mark legacy endpoints deprecated (removal date: Nov 30)  
  - Promote new event filtering parameters

## Content Plan

- Draft in `docs/api/index.md` (repo main branch)
- Route review to Priya (PM) and Allison (Support) by Nov 1
- Schedule staging validation with QA once docs land
- Publish changelog entry + broadcast via customer newsletter

## Resources

- Previous release notes template (`work/onboarding-resources`)
- Loom walkthrough from Ken on new dashboard flow
