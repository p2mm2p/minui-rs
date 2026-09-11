/*
 * mock_core.c — minarch FFI 测试用 mock libretro 核心
 *
 * 用途: 在 macOS/Linux 主机端编译为动态库, 供 minarch/tests/core_loading.rs
 * 验证 libretro.rs 的 Core 加载器与回调桥(无需真实掌机硬件)。
 *
 * 编译: 由 tests/common/mod.rs 经 cc crate 编译(见该文件)。
 *
 * 行为约定(与 tests/core_loading.rs 与 tests/core.rs 断言一一对应):
 * - retro_get_system_info: library_name="mock_core", library_version="0.0.1",
 *   valid_extensions="mc|zip", need_fullpath=false
 * - retro_get_system_av_info: 160x144 @60fps / 44100Hz, aspect=160/144
 * - retro_run: 触发 video_refresh(160x144, pitch 320, 首 2 字节 0x1234)
 *   与 audio_sample_batch(735 帧)
 * - retro_serialize_size: 8; retro_serialize 填 0xAB
 * - retro_get_memory_data/size: SAVE_RAM 16 字节 / RTC 8 字节静态缓冲
 *   (供 CoreSession 的 sram 接线端到端测试)
 *
 * 编译宏(测行为分支与缺失符号路径):
 * - MOCK_NO_RETRO_RUN            不导出 retro_run
 * - MOCK_NO_LOAD_GAME_SPECIAL    不导出 retro_load_game_special
 * - MOCK_NO_GET_REGION           不导出 retro_get_region
 * - MOCK_LOAD_GAME_FAIL          retro_load_game 返回 0(加载失败路径)
 * - MOCK_ZERO_ASPECT             av_info.aspect_ratio 填 0.0(宽高比回落路径)
 * - MOCK_TRACK_CALLS             retro_unload_game/retro_deinit/
 *                                retro_set_controller_port_device 计数,
 *                                经 mock_call_count(name) 查询
 */

#include "libretro.h"

#include <string.h>

/* ── 调用计数(MOCK_TRACK_CALLS) ─────────────────────────────── */

#ifdef MOCK_TRACK_CALLS
static unsigned unload_game_calls;
static unsigned deinit_calls;
static unsigned set_controller_calls;
#endif

/* 按符号名查询调用次数(未启用计数时恒 0)——供 Rust 测试断言调用序 */
unsigned mock_call_count(const char *name)
{
#ifdef MOCK_TRACK_CALLS
   if (strcmp(name, "retro_unload_game") == 0) return unload_game_calls;
   if (strcmp(name, "retro_deinit") == 0) return deinit_calls;
   if (strcmp(name, "retro_set_controller_port_device") == 0) return set_controller_calls;
#else
   (void)name;
#endif
   return 0;
}

/* ── 前端回调指针(由 retro_set_* 注入) ───────────────────────── */

static retro_environment_t env_cb;
static retro_video_refresh_t video_cb;
static retro_audio_sample_t audio_cb;
static retro_audio_sample_batch_t audio_batch_cb;
static retro_input_poll_t input_poll_cb;
static retro_input_state_t input_state_cb;

/* ── 测试帧/音频数据 ──────────────────────────────────────────── */

static uint16_t frame_data[160 * 144] = {0x1234};   /* 静态初始化，与 retro_init 顺序解耦 */
static int16_t audio_samples[735 * 2];              /* 735 帧立体声 */

/* ── 生命周期 ─────────────────────────────────────────────────── */

void retro_init(void)
{
}

void retro_deinit(void)
{
#ifdef MOCK_TRACK_CALLS
   deinit_calls++;
#endif
}

/* ── 系统信息 ─────────────────────────────────────────────────── */

void retro_get_system_info(struct retro_system_info *info)
{
   info->library_name     = "mock_core";
   info->library_version  = "0.0.1";
   info->valid_extensions = "mc|zip";
   info->need_fullpath    = false;
   info->block_extract    = false;
}

void retro_get_system_av_info(struct retro_system_av_info *info)
{
   info->geometry.base_width   = 160;
   info->geometry.base_height  = 144;
   info->geometry.max_width    = 160;
   info->geometry.max_height   = 144;
#ifdef MOCK_ZERO_ASPECT
   info->geometry.aspect_ratio = 0.0f;
#else
   info->geometry.aspect_ratio = 160.0f / 144.0f;
#endif
   info->timing.fps            = 60.0;
   info->timing.sample_rate    = 44100.0;
}

/* ── 控制器 ───────────────────────────────────────────────────── */

void retro_set_controller_port_device(unsigned port, unsigned device)
{
   (void)port;
   (void)device;
#ifdef MOCK_TRACK_CALLS
   set_controller_calls++;
#endif
}

/* ── 运行 ─────────────────────────────────────────────────────── */

void retro_reset(void)
{
}

#ifndef MOCK_NO_RETRO_RUN
void retro_run(void)
{
   if (video_cb)
      video_cb(frame_data, 160, 144, 320);
   if (audio_batch_cb)
      audio_batch_cb(audio_samples, 735);
}
#endif

/* ── 存档状态(序列化) ─────────────────────────────────────────── */

size_t retro_serialize_size(void)
{
   return 8;
}

bool retro_serialize(void *data, size_t len)
{
   if (!data || len < 8)
      return false;
   memset(data, 0xAB, len);
   return true;
}

bool retro_unserialize(const void *data, size_t len)
{
   return data != NULL && len == 8;
}

/* ── 游戏加载 ─────────────────────────────────────────────────── */

bool retro_load_game(const struct retro_game_info *game)
{
#ifdef MOCK_LOAD_GAME_FAIL
   (void)game;
   return false;
#else
   return game != NULL;
#endif
}

#ifndef MOCK_NO_LOAD_GAME_SPECIAL
bool retro_load_game_special(unsigned game_type,
      const struct retro_game_info *info, size_t num_info)
{
   return game_type != 0 && info != NULL && num_info != 0;
}
#endif

void retro_unload_game(void)
{
#ifdef MOCK_TRACK_CALLS
   unload_game_calls++;
#endif
}

#ifndef MOCK_NO_GET_REGION
unsigned retro_get_region(void)
{
   return RETRO_REGION_NTSC;
}
#endif

/* ── 内存 ─────────────────────────────────────────────────────── */

/* 核心电池存档内存(16 字节 SRAM + 8 字节 RTC)——供 CoreSession 的
   sram 接线端到端测试: load 读档灌入、quit 写档取走 */
#define MOCK_SRAM_SIZE 16
#define MOCK_RTC_SIZE 8
static uint8_t sram_data[MOCK_SRAM_SIZE];
static uint8_t rtc_data[MOCK_RTC_SIZE];

void *retro_get_memory_data(unsigned id)
{
   if (id == RETRO_MEMORY_SAVE_RAM) return sram_data;
   if (id == RETRO_MEMORY_RTC) return rtc_data;
   return NULL;
}

size_t retro_get_memory_size(unsigned id)
{
   if (id == RETRO_MEMORY_SAVE_RAM) return MOCK_SRAM_SIZE;
   if (id == RETRO_MEMORY_RTC) return MOCK_RTC_SIZE;
   return 0;
}

/* ── 回调注入 ─────────────────────────────────────────────────── */

void retro_set_environment(retro_environment_t cb)
{
   env_cb = cb;
}

void retro_set_video_refresh(retro_video_refresh_t cb)
{
   video_cb = cb;
}

void retro_set_audio_sample(retro_audio_sample_t cb)
{
   audio_cb = cb;
}

void retro_set_audio_sample_batch(retro_audio_sample_batch_t cb)
{
   audio_batch_cb = cb;
}

void retro_set_input_poll(retro_input_poll_t cb)
{
   input_poll_cb = cb;
}

void retro_set_input_state(retro_input_state_t cb)
{
   input_state_cb = cb;
}
