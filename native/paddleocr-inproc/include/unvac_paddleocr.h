#pragma once

#include <stdint.h>

#if defined(_WIN32)
#  if defined(UNVAC_PADDLEOCR_BUILD_DLL)
#    define UNVAC_PADDLEOCR_API __declspec(dllexport)
#  else
#    define UNVAC_PADDLEOCR_API __declspec(dllimport)
#  endif
#else
#  define UNVAC_PADDLEOCR_API __attribute__((visibility("default")))
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef struct UnvacPaddleOcrEngine UnvacPaddleOcrEngine;

typedef enum UnvacPaddleOcrStatus {
  UNVAC_PADDLEOCR_OK = 0,
  UNVAC_PADDLEOCR_NOT_READY = 1,
  UNVAC_PADDLEOCR_INVALID_ARGUMENT = 2,
  UNVAC_PADDLEOCR_RUNTIME_ERROR = 3
} UnvacPaddleOcrStatus;

typedef struct UnvacPaddleOcrImage {
  const uint8_t* data;
  uint32_t width;
  uint32_t height;
  uint32_t stride;
  uint32_t channels;
} UnvacPaddleOcrImage;

typedef struct UnvacPaddleOcrOptions {
  const char* language;
  int32_t roi_x;
  int32_t roi_y;
  int32_t roi_w;
  int32_t roi_h;
  uint32_t return_regions;
} UnvacPaddleOcrOptions;

typedef struct UnvacPaddleOcrText {
  const char* text;
  uint32_t len;
} UnvacPaddleOcrText;

UNVAC_PADDLEOCR_API uint32_t unvac_paddleocr_abi_version(void);
UNVAC_PADDLEOCR_API UnvacPaddleOcrStatus unvac_paddleocr_engine_create(
  const char* model_dir,
  UnvacPaddleOcrEngine** out_engine
);
UNVAC_PADDLEOCR_API void unvac_paddleocr_engine_destroy(UnvacPaddleOcrEngine* engine);
UNVAC_PADDLEOCR_API UnvacPaddleOcrStatus unvac_paddleocr_recognize(
  UnvacPaddleOcrEngine* engine,
  const UnvacPaddleOcrImage* image,
  const UnvacPaddleOcrOptions* options,
  UnvacPaddleOcrText* out_text
);
UNVAC_PADDLEOCR_API void unvac_paddleocr_text_free(UnvacPaddleOcrText text);
UNVAC_PADDLEOCR_API const char* unvac_paddleocr_last_error(void);

#ifdef __cplusplus
}
#endif
