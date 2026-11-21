cmake_minimum_required(VERSION 3.20)
project(unvac_paddleocr LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

option(UNVAC_PADDLEOCR_WITH_UPSTREAM "Build against PaddleOCR/Paddle Inference instead of the stub implementation" OFF)
set(UNVAC_PADDLEOCR_VENDOR_ROOT "" CACHE PATH "Generated PaddleOCR native vendor workspace")
set(UNVAC_PADDLEOCR_OPENCV_DIR "" CACHE PATH "OpenCV install directory")
set(UNVAC_PADDLEOCR_INFERENCE_DIR "" CACHE PATH "Paddle Inference install directory")

add_library(unvac_paddleocr SHARED
  src/unvac_paddleocr.cpp
)

target_include_directories(unvac_paddleocr
  PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_compile_definitions(unvac_paddleocr
  PRIVATE
    UNVAC_PADDLEOCR_BUILD_DLL=1
)

if(UNVAC_PADDLEOCR_WITH_UPSTREAM)
  if(NOT DEFINED UNVAC_PADDLEOCR_VENDOR_ROOT)
    message(FATAL_ERROR "UNVAC_PADDLEOCR_VENDOR_ROOT is required")
  endif()
  if(NOT DEFINED UNVAC_PADDLEOCR_OPENCV_DIR)
    message(FATAL_ERROR "UNVAC_PADDLEOCR_OPENCV_DIR is required")
  endif()
  if(NOT DEFINED UNVAC_PADDLEOCR_INFERENCE_DIR)
    message(FATAL_ERROR "UNVAC_PADDLEOCR_INFERENCE_DIR is required")
  endif()

  target_compile_definitions(unvac_paddleocr PRIVATE UNVAC_PADDLEOCR_WITH_UPSTREAM=1)
  target_include_directories(unvac_paddleocr PRIVATE
    ${UNVAC_PADDLEOCR_VENDOR_ROOT}/src/PaddleOCR/deploy/cpp_infer/include
    ${UNVAC_PADDLEOCR_OPENCV_DIR}/include
    ${UNVAC_PADDLEOCR_INFERENCE_DIR}/paddle/include
  )

  # Link directories and library names vary across Paddle Inference/OpenCV packages.
  # Keep this intentionally explicit in the generated build cache until the exact
  # release package is pinned.
endif()

install(TARGETS unvac_paddleocr
  RUNTIME DESTINATION bin
  LIBRARY DESTINATION lib
  ARCHIVE DESTINATION lib
)
install(DIRECTORY include/ DESTINATION include)
