# ai-eval

소형 LLM이 유효한 DesiredState를 만드는지 검증하는 하네스.

## 기본 테스트 (Mock, 결정론)

    cargo test -p ai-eval

## 실제 모델 run (Ollama gemma4:e4b)

    AI_BASE_URL=http://localhost:11434/v1 AI_API_KEY=ollama AI_MODEL=gemma4:e4b \
      cargo run -p ai-eval --features openai-client

각 fixture가 parse/validate/compile/diff/graph 중 어디까지 도달하는지 리포트한다.
graphed 비율이 높을수록 소형 모델이 파이프라인을 끝까지 통과시킨 것.
