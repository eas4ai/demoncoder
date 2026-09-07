# Reliability mechanism and implementation review

Status: in progress

## REL-001 mechanism design

A disposable Unix listener under a private home fixture directory is outside the
selected workspace and outside the sandbox's hidden /tmp. The production executor
attempts one connection; the service has no privileged operation. The client must
report denial and the listener must see no accepted connection. Existing tests
retain outside-write/private-file canaries and ordinary IP networking, Git and
installed tool behavior. Baseline and correction results will follow.
