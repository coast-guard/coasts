# 레시피

레시피는 일반적인 프로젝트 형태를 위한 완전하고 주석이 달린 Coastfile 예제입니다. 각 레시피에는 복사하여 수정할 수 있는 전체 Coastfile이 포함되어 있으며, 이어서 각 결정이 내려진 이유를 설명하는 섹션별 단계별 안내가 제공됩니다.

Coastfile이 처음이라면 먼저 [Coastfiles reference](../coastfiles/README.md)부터 시작하세요. 레시피는 핵심 개념에 대한 이해를 전제로 합니다.

- [Full-Stack Monorepo](FULLSTACK_MONOREPO.md) - 호스트에서 공유되는 Postgres와 Redis, bare-service Vite 프런트엔드, 그리고 compose를 통한 도커라이즈된 백엔드. 볼륨 전략, 헬스체크, assign 튜닝, 그리고 대규모 저장소를 위한 `exclude_paths`를 다룹니다.
- [Next.js Application](NEXTJS.md) - Turbopack을 사용하는 Next.js, 공유 Postgres와 Redis, 백그라운드 워커, 그리고 인증 콜백을 위한 동적 포트 처리. `.next` 격리를 위한 `private_paths`, bare service 최적화, 그리고 멀티 에이전트 워크트리 지원을 다룹니다.
