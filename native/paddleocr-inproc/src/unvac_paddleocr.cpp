#include "unvac_paddleocr.h"

#include <string>

namespace {
thread_local std::string last_error;

void set_error(const char* message) {
  last_error = message == nullptr ? "" : message;
}
}

struct UnvacPaddleOcrEngine {
  std::string model_dir;
};

uint32_t unvac_paddleocr_abi_version(void) {
  return 1;
}

UnvacPaddleOcrStatus unvac_paddleocr_engine_create(
  const char* model_dir,
  UnvacPaddleOcrEngine** out_engine
) {
  if (out_engine == nullptr) {
    set_error("out_engine is null");
    return UNVAC_PADDLEOCR_INVALID_ARGUMENT;
  }
  *out_engine = nullptr;
  if (model_dir == nullptr || model_dir[0] == '\0') {
    set_error("model_dir is empty");
    return UNVAC_PADDLEOCR_INVALID_ARGUMENT;
  }

#if defined(UNVAC_PADDLEOCR_WITH_UPSTREAM)
  auto* engine = new UnvacPaddleOcrEngine();
  engine->model_dir = model_dir;
  *out_engine = engine;
  set_error("");
  return UNVAC_PADDLEOCR_OK;
#else
  set_error("PaddleOCR upstream integration is not enabled in this experimental build");
  return UNVAC_PADDLEOCR_NOT_READY;
#endif
}

void unvac_paddleocr_engine_destroy(UnvacPaddleOcrEngine* engine) {
  delete engine;
}

UnvacPaddleOcrStatus unvac_paddleocr_recognize(
  UnvacPaddleOcrEngine* engine,
  const UnvacPaddleOcrImage* image,
  const UnvacPaddleOcrOptions* options,
  UnvacPaddleOcrText* out_text
) {
  (void)engine;
  (void)image;
  (void)options;
  if (out_text == nullptr) {
    set_error("out_text is null");
    return UNVAC_PADDLEOCR_INVALID_ARGUMENT;
  }
  out_text->text = nullptr;
  out_text->len = 0;

#if defined(UNVAC_PADDLEOCR_WITH_UPSTREAM)
  set_error("PaddleOCR recognize bridge is not wired yet");
  return UNVAC_PADDLEOCR_NOT_READY;
#else
  set_error("PaddleOCR upstream integration is not enabled in this experimental build");
  return UNVAC_PADDLEOCR_NOT_READY;
#endif
}

void unvac_paddleocr_text_free(UnvacPaddleOcrText text) {
  (void)text;
}

const char* unvac_paddleocr_last_error(void) {
  return last_error.c_str();
}
